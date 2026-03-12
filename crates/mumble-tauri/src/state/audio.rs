//! Audio / voice pipeline management: enable, disable, mute, deafen,
//! and the background outbound audio loop.
//!
//! On Android, audio hardware access via cpal is not available.  The
//! voice commands gracefully return errors so the frontend can
//! disable audio controls.

use tauri::Emitter;

use super::types::VoiceState;
use super::AppState;

// ── Shared methods (all platforms) ────────────────────────────────

impl AppState {
    /// Get current audio settings.
    pub fn audio_settings(&self) -> super::types::AudioSettings {
        self.inner
            .lock()
            .map(|s| s.audio_settings.clone())
            .unwrap_or_default()
    }

    /// Update audio settings.
    pub fn set_audio_settings(&self, settings: super::types::AudioSettings) {
        if let Ok(mut state) = self.inner.lock() {
            state.audio_settings = settings;
        }
    }

    /// Get current voice state.
    pub fn voice_state(&self) -> VoiceState {
        self.inner
            .lock()
            .map(|s| s.voice_state)
            .unwrap_or_default()
    }

    /// Emit voice-state-changed event to the frontend.
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(super) fn emit_voice_state(&self) {
        if let Some(app) = self.app_handle() {
            let vs = self.voice_state();
            let _ = app.emit("voice-state-changed", vs);
        }
    }
}

// ── Desktop: full cpal-based audio pipeline ───────────────────────

#[cfg(not(target_os = "android"))]
mod desktop {
    use std::time::Duration;

    use tracing::{info, warn};

    use mumble_protocol::audio::decoder::OpusDecoder;
    use mumble_protocol::audio::encoder::{OpusEncoder, OpusEncoderConfig};
    use mumble_protocol::audio::filter::automatic_gain::AutomaticGainControl;
    use mumble_protocol::audio::filter::noise_gate::{NoiseGate, NoiseGateConfig};
    use mumble_protocol::audio::filter::FilterChain;
    use mumble_protocol::audio::pipeline::{InboundPipeline, OutboundPipeline, OutboundTick};
    use mumble_protocol::audio::sample::AudioFormat;
    use mumble_protocol::client::ClientHandle;
    use mumble_protocol::command;

    use crate::audio::{CpalCapture, CpalPlayback};
    use crate::state::types::{AudioSettings, VoiceState};
    use crate::state::AppState;

    impl AppState {
        /// Enable voice calling: unmute + undeaf, start audio pipelines.
        pub async fn enable_voice(&self) -> Result<(), String> {
            let (handle, audio_settings) = {
                let state = self.inner.lock().map_err(|e| e.to_string())?;
                (state.client_handle.clone(), state.audio_settings.clone())
            };

            info!("enable_voice: starting audio pipelines");

            // Inbound: network -> decoder -> playback.
            let playback = CpalPlayback::new().map_err(|e| format!("Playback init: {e}"))?;
            let decoder = OpusDecoder::new(AudioFormat::MONO_48KHZ_F32)
                .map_err(|e| format!("Decoder init: {e}"))?;
            let mut inbound = InboundPipeline::new(
                Box::new(decoder),
                FilterChain::new(),
                Box::new(playback),
            );
            inbound.start().map_err(|e| format!("Playback start: {e}"))?;

            // Outbound: capture -> filters -> encoder -> network.
            let capture = CpalCapture::new(
                audio_settings.selected_device.as_deref(),
                960, // 20 ms @ 48 kHz (matches encoder frame size)
            )
            .map_err(|e| format!("Capture init: {e}"))?;

            let encoder_config = OpusEncoderConfig::default();
            let encoder = OpusEncoder::new(encoder_config, AudioFormat::MONO_48KHZ_F32)
                .map_err(|e| format!("Encoder init: {e}"))?;

            let mut outbound_filters = FilterChain::new();
            let noise_gate = NoiseGate::new(NoiseGateConfig {
                open_threshold: audio_settings.vad_threshold,
                close_threshold: audio_settings.vad_threshold * audio_settings.noise_gate_close_ratio,
                hold_frames: audio_settings.hold_frames,
                ..NoiseGateConfig::default()
            });
            outbound_filters.push(Box::new(noise_gate));
            if audio_settings.auto_gain {
                outbound_filters
                    .push(Box::new(AutomaticGainControl::new(Default::default())));
            }

            let mut outbound = OutboundPipeline::new(
                Box::new(capture),
                outbound_filters,
                Box::new(encoder),
            );
            outbound.start().map_err(|e| format!("Capture start: {e}"))?;

            let outbound_handle = if let Some(ref client) = handle {
                let client = client.clone();
                Some(tokio::spawn(async move {
                    outbound_audio_loop(outbound, client).await;
                }))
            } else {
                None
            };

            {
                let mut state = self.inner.lock().map_err(|e| e.to_string())?;
                state.voice_state = VoiceState::Active;
                state.inbound_pipeline = Some(inbound);
                state.outbound_task_handle = outbound_handle;
            }

            info!("enable_voice: pipelines started, sending unmute");

            if let Some(handle) = handle {
                handle
                    .send(command::SetSelfMute { muted: false })
                    .await
                    .map_err(|e| format!("Failed to unmute: {e}"))?;
            }

            self.emit_voice_state();
            Ok(())
        }

        /// Disable voice calling: go back to deaf + muted, stop pipelines.
        pub async fn disable_voice(&self) -> Result<(), String> {
            self.stop_audio();

            let handle = {
                let mut state = self.inner.lock().map_err(|e| e.to_string())?;
                state.voice_state = VoiceState::Inactive;
                state.client_handle.clone()
            };

            if let Some(handle) = handle {
                handle
                    .send(command::SetSelfDeaf { deafened: true })
                    .await
                    .map_err(|e| format!("Failed to deafen: {e}"))?;
            }

            self.emit_voice_state();
            Ok(())
        }

        /// Stop all running audio pipelines and tasks.
        pub(in crate::state) fn stop_audio(&self) {
            if let Ok(mut state) = self.inner.lock() {
                if let Some(handle) = state.outbound_task_handle.take() {
                    handle.abort();
                }
                state.inbound_pipeline = None;
            }
        }

        /// Stop only the outbound (mic capture) pipeline.
        fn stop_outbound(&self) {
            if let Ok(mut state) = self.inner.lock() {
                if let Some(handle) = state.outbound_task_handle.take() {
                    handle.abort();
                }
            }
        }

        /// Create and start a fresh outbound audio pipeline (mic -> encoder -> network).
        fn start_outbound_pipeline(
            &self,
            audio_settings: &AudioSettings,
            client_handle: &Option<ClientHandle>,
        ) -> Result<(), String> {
            let capture = CpalCapture::new(
                audio_settings.selected_device.as_deref(),
                960,
            )
            .map_err(|e| format!("Capture init: {e}"))?;

            let encoder = OpusEncoder::new(OpusEncoderConfig::default(), AudioFormat::MONO_48KHZ_F32)
                .map_err(|e| format!("Encoder init: {e}"))?;

            let mut outbound_filters = FilterChain::new();
            outbound_filters.push(Box::new(NoiseGate::new(NoiseGateConfig {
                open_threshold: audio_settings.vad_threshold,
                close_threshold: audio_settings.vad_threshold * audio_settings.noise_gate_close_ratio,
                hold_frames: audio_settings.hold_frames,
                ..NoiseGateConfig::default()
            })));
            if audio_settings.auto_gain {
                outbound_filters.push(Box::new(AutomaticGainControl::new(Default::default())));
            }

            let mut outbound = OutboundPipeline::new(
                Box::new(capture),
                outbound_filters,
                Box::new(encoder),
            );
            outbound.start().map_err(|e| format!("Capture start: {e}"))?;

            let outbound_handle = if let Some(ref client) = client_handle {
                let client = client.clone();
                Some(tokio::spawn(async move {
                    outbound_audio_loop(outbound, client).await;
                }))
            } else {
                None
            };

            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.outbound_task_handle = outbound_handle;
            Ok(())
        }

        /// Toggle mute: Active <-> Muted.
        pub async fn toggle_mute(&self) -> Result<(), String> {
            let (voice_state, handle, audio_settings) = {
                let state = self.inner.lock().map_err(|e| e.to_string())?;
                (
                    state.voice_state,
                    state.client_handle.clone(),
                    state.audio_settings.clone(),
                )
            };

            match voice_state {
                VoiceState::Active => {
                    info!("toggle_mute: muting (stopping outbound)");
                    self.stop_outbound();
                    {
                        let mut state = self.inner.lock().map_err(|e| e.to_string())?;
                        state.voice_state = VoiceState::Muted;
                    }
                    if let Some(handle) = handle {
                        handle
                            .send(command::SetSelfMute { muted: true })
                            .await
                            .map_err(|e| format!("Failed to mute: {e}"))?;
                    }
                }
                VoiceState::Muted => {
                    info!("toggle_mute: unmuting (restarting outbound)");
                    self.start_outbound_pipeline(&audio_settings, &handle)?;
                    {
                        let mut state = self.inner.lock().map_err(|e| e.to_string())?;
                        state.voice_state = VoiceState::Active;
                    }
                    if let Some(handle) = handle {
                        handle
                            .send(command::SetSelfMute { muted: false })
                            .await
                            .map_err(|e| format!("Failed to unmute: {e}"))?;
                    }
                }
                VoiceState::Inactive => {
                    return Ok(());
                }
            }

            self.emit_voice_state();
            Ok(())
        }

        /// Toggle deafen: Active/Muted -> Inactive, Inactive -> Active.
        pub async fn toggle_deafen(&self) -> Result<(), String> {
            let voice_state = {
                let state = self.inner.lock().map_err(|e| e.to_string())?;
                state.voice_state
            };

            match voice_state {
                VoiceState::Active | VoiceState::Muted => {
                    self.disable_voice().await?;
                }
                VoiceState::Inactive => {
                    self.enable_voice().await?;
                }
            }
            Ok(())
        }
    }

    /// Background task that reads from the microphone, encodes, and sends
    /// Opus packets to the server via the client handle.
    async fn outbound_audio_loop(mut pipeline: OutboundPipeline, handle: ClientHandle) {
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        let mut packet_count: u64 = 0;

        loop {
            interval.tick().await;

            loop {
                match pipeline.tick() {
                    Ok(OutboundTick::Audio(packet)) => {
                        packet_count += 1;
                        if packet_count == 1 || packet_count.is_multiple_of(500) {
                            info!(
                                "outbound_audio: sending packet #{} (opus {} bytes, seq {})",
                                packet_count,
                                packet.data.len(),
                                packet.sequence
                            );
                        }
                        if let Err(e) = handle
                            .send(command::SendAudio {
                                opus_data: packet.data,
                                target: 0,
                                frame_number: packet.sequence,
                                positional_data: None,
                                is_terminator: false,
                            })
                            .await
                        {
                            warn!("outbound_audio: send failed: {e}");
                        }
                    }
                    Ok(OutboundTick::Terminator(packet)) => {
                        info!(
                            "outbound_audio: sending terminator (opus {} bytes)",
                            packet.data.len()
                        );
                        if let Err(e) = handle
                            .send(command::SendAudio {
                                opus_data: packet.data,
                                target: 0,
                                frame_number: packet.sequence,
                                positional_data: None,
                                is_terminator: true,
                            })
                            .await
                        {
                            warn!("outbound_audio: terminator send failed: {e}");
                        }
                    }
                    Ok(OutboundTick::Silence) => {
                        continue;
                    }
                    Ok(OutboundTick::NoData) => break,
                    Err(e) => {
                        warn!("outbound audio error: {e}");
                        break;
                    }
                }
            }
        }
    }
}

// ── Android: stub audio (no cpal) ─────────────────────────────────

#[cfg(target_os = "android")]
impl AppState {
    /// Audio is not yet supported on Android.
    pub async fn enable_voice(&self) -> Result<(), String> {
        Err("Audio is not yet supported on Android".into())
    }

    /// Audio is not yet supported on Android.
    pub async fn disable_voice(&self) -> Result<(), String> {
        tracing::info!("disable_voice: no-op on Android");
        Ok(())
    }

    /// No-op on Android (no audio pipelines to stop).
    pub(super) fn stop_audio(&self) {}

    /// Audio is not yet supported on Android.
    pub async fn toggle_mute(&self) -> Result<(), String> {
        Err("Audio is not yet supported on Android".into())
    }

    /// Audio is not yet supported on Android.
    pub async fn toggle_deafen(&self) -> Result<(), String> {
        Err("Audio is not yet supported on Android".into())
    }
}
