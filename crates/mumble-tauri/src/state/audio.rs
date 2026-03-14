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
    ///
    /// If any pipeline-relevant setting changed while voice is active,
    /// the outbound pipeline is automatically restarted.
    /// Volume changes are applied live via atomic handles (no restart).
    pub fn set_audio_settings(&self, settings: super::types::AudioSettings) -> Option<(bool, bool)> {
        let (old_settings, voice_active) = {
            let state = self.inner.lock().ok()?;
            (
                state.audio_settings.clone(),
                state.voice_state == VoiceState::Active,
            )
        };

        let restart_outbound = voice_active && old_settings.needs_pipeline_restart(&settings);
        let restart_inbound = voice_active && old_settings.needs_inbound_restart(&settings);

        // Update live volume handles (no pipeline restart needed).
        #[cfg(not(target_os = "android"))]
        if let Ok(state) = self.inner.lock() {
            use std::sync::atomic::Ordering;
            if let Some(ref h) = state.input_volume_handle {
                h.store(settings.input_volume.to_bits(), Ordering::Relaxed);
            }
            if let Some(ref h) = state.output_volume_handle {
                h.store(settings.output_volume.to_bits(), Ordering::Relaxed);
            }
        }

        if let Ok(mut state) = self.inner.lock() {
            state.audio_settings = settings;
        }

        Some((restart_outbound, restart_inbound))
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
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;
    use std::time::Duration;

    use tracing::{info, warn};

    use mumble_protocol::audio::decoder::OpusDecoder;
    use mumble_protocol::audio::encoder::{OpusEncoder, OpusEncoderConfig};
    use mumble_protocol::audio::filter::automatic_gain::{AgcConfig, AutomaticGainControl};
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

            // Create shared volume handles for live updates.
            let input_vol = Arc::new(AtomicU32::new(audio_settings.input_volume.to_bits()));
            let output_vol = Arc::new(AtomicU32::new(audio_settings.output_volume.to_bits()));

            // Inbound: network -> decoder -> playback.
            let playback = CpalPlayback::new(
                audio_settings.selected_output_device.as_deref(),
                output_vol.clone(),
            )
            .map_err(|e| format!("Playback init: {e}"))?;
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
                audio_settings.frame_size_samples(),
                input_vol.clone(),
            )
            .map_err(|e| format!("Capture init: {e}"))?;

            let encoder_config = OpusEncoderConfig {
                bitrate: audio_settings.bitrate_bps,
                frame_size: audio_settings.frame_size_samples(),
                ..OpusEncoderConfig::default()
            };
            let encoder = OpusEncoder::new(encoder_config, AudioFormat::MONO_48KHZ_F32)
                .map_err(|e| format!("Encoder init: {e}"))?;

            let mut outbound_filters = FilterChain::new();
            if audio_settings.noise_suppression {
                let noise_gate = NoiseGate::new(NoiseGateConfig {
                    open_threshold: audio_settings.vad_threshold,
                    close_threshold: audio_settings.vad_threshold * audio_settings.noise_gate_close_ratio,
                    hold_frames: audio_settings.hold_frames,
                    ..NoiseGateConfig::default()
                });
                outbound_filters.push(Box::new(noise_gate));
            }
            if audio_settings.auto_gain {
                let max_gain_linear = 10.0_f32.powf(audio_settings.max_gain_db / 20.0);
                outbound_filters.push(Box::new(AutomaticGainControl::new(AgcConfig {
                    max_gain: max_gain_linear,
                    ..AgcConfig::default()
                })));
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
                state.input_volume_handle = Some(input_vol);
                state.output_volume_handle = Some(output_vol);
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
                state.input_volume_handle = None;
                state.output_volume_handle = None;
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

        /// Restart the outbound pipeline with the current audio settings.
        ///
        /// Called when the input device (or other capture-relevant settings)
        /// change while voice is active.
        pub fn restart_outbound(&self) -> Result<(), String> {
            info!("restart_outbound: restarting capture pipeline with new settings");
            self.stop_outbound();

            let (audio_settings, client_handle) = {
                let state = self.inner.lock().map_err(|e| e.to_string())?;
                (state.audio_settings.clone(), state.client_handle.clone())
            };

            self.start_outbound_pipeline(&audio_settings, &client_handle)
        }

        /// Restart the inbound pipeline with the current audio settings.
        ///
        /// Called when the output device changes while voice is active.
        pub fn restart_inbound(&self) -> Result<(), String> {
            info!("restart_inbound: restarting playback pipeline with new settings");

            // Stop the old inbound pipeline.
            if let Ok(mut state) = self.inner.lock() {
                state.inbound_pipeline = None;
            }

            let audio_settings = {
                let state = self.inner.lock().map_err(|e| e.to_string())?;
                state.audio_settings.clone()
            };

            let output_vol = Arc::new(AtomicU32::new(audio_settings.output_volume.to_bits()));

            let playback = CpalPlayback::new(
                audio_settings.selected_output_device.as_deref(),
                output_vol.clone(),
            )
            .map_err(|e| format!("Playback init: {e}"))?;
            let decoder = OpusDecoder::new(AudioFormat::MONO_48KHZ_F32)
                .map_err(|e| format!("Decoder init: {e}"))?;
            let mut inbound = InboundPipeline::new(
                Box::new(decoder),
                FilterChain::new(),
                Box::new(playback),
            );
            inbound.start().map_err(|e| format!("Playback start: {e}"))?;

            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.inbound_pipeline = Some(inbound);
            state.output_volume_handle = Some(output_vol);
            Ok(())
        }

        /// Create and start a fresh outbound audio pipeline (mic -> encoder -> network).
        fn start_outbound_pipeline(
            &self,
            audio_settings: &AudioSettings,
            client_handle: &Option<ClientHandle>,
        ) -> Result<(), String> {
            // Re-use existing input volume handle or create a new one.
            let input_vol = {
                let state = self.inner.lock().map_err(|e| e.to_string())?;
                state.input_volume_handle.clone()
            }
            .unwrap_or_else(|| Arc::new(AtomicU32::new(audio_settings.input_volume.to_bits())));

            let capture = CpalCapture::new(
                audio_settings.selected_device.as_deref(),
                audio_settings.frame_size_samples(),
                input_vol.clone(),
            )
            .map_err(|e| format!("Capture init: {e}"))?;

            let encoder_config = OpusEncoderConfig {
                bitrate: audio_settings.bitrate_bps,
                frame_size: audio_settings.frame_size_samples(),
                ..OpusEncoderConfig::default()
            };
            let encoder = OpusEncoder::new(encoder_config, AudioFormat::MONO_48KHZ_F32)
                .map_err(|e| format!("Encoder init: {e}"))?;

            let mut outbound_filters = FilterChain::new();
            if audio_settings.noise_suppression {
                outbound_filters.push(Box::new(NoiseGate::new(NoiseGateConfig {
                    open_threshold: audio_settings.vad_threshold,
                    close_threshold: audio_settings.vad_threshold * audio_settings.noise_gate_close_ratio,
                    hold_frames: audio_settings.hold_frames,
                    ..NoiseGateConfig::default()
                })));
            }
            if audio_settings.auto_gain {
                let max_gain_linear = 10.0_f32.powf(audio_settings.max_gain_db / 20.0);
                outbound_filters.push(Box::new(AutomaticGainControl::new(AgcConfig {
                    max_gain: max_gain_linear,
                    ..AgcConfig::default()
                })));
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
            state.input_volume_handle = Some(input_vol);
            Ok(())
        }

        /// Enable voice in muted state: start inbound pipeline (hearing)
        /// but keep outbound stopped (mic off).
        ///
        /// Used when undeafening from Inactive to land in Muted state.
        pub async fn enable_voice_muted(&self) -> Result<(), String> {
            let (handle, audio_settings) = {
                let state = self.inner.lock().map_err(|e| e.to_string())?;
                (state.client_handle.clone(), state.audio_settings.clone())
            };

            info!("enable_voice_muted: starting inbound pipeline only");

            let output_vol = Arc::new(AtomicU32::new(audio_settings.output_volume.to_bits()));

            let playback = CpalPlayback::new(
                audio_settings.selected_output_device.as_deref(),
                output_vol.clone(),
            )
            .map_err(|e| format!("Playback init: {e}"))?;
            let decoder = OpusDecoder::new(AudioFormat::MONO_48KHZ_F32)
                .map_err(|e| format!("Decoder init: {e}"))?;
            let mut inbound = InboundPipeline::new(
                Box::new(decoder),
                FilterChain::new(),
                Box::new(playback),
            );
            inbound.start().map_err(|e| format!("Playback start: {e}"))?;

            {
                let mut state = self.inner.lock().map_err(|e| e.to_string())?;
                state.voice_state = VoiceState::Muted;
                state.inbound_pipeline = Some(inbound);
                state.output_volume_handle = Some(output_vol);
            }

            info!("enable_voice_muted: inbound started, sending undeafen");

            if let Some(handle) = handle {
                handle
                    .send(command::SetSelfDeaf { deafened: false })
                    .await
                    .map_err(|e| format!("Failed to undeafen: {e}"))?;
                handle
                    .send(command::SetSelfMute { muted: true })
                    .await
                    .map_err(|e| format!("Failed to mute: {e}"))?;
            }

            self.emit_voice_state();
            Ok(())
        }

        /// Toggle mute.
        ///
        /// | Current   | Result  |
        /// |-----------|---------|
        /// | Inactive  | Active  | (full unmute + undeaf)
        /// | Active    | Muted   |
        /// | Muted     | Active  |
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
                    info!("toggle_mute: enabling voice from inactive");
                    self.enable_voice().await?;
                    return Ok(());
                }
            }

            self.emit_voice_state();
            Ok(())
        }

        /// Toggle deafen.
        ///
        /// | Current   | Result   |
        /// |-----------|----------|
        /// | Inactive  | Muted    | (undeaf, stay muted)
        /// | Active    | Inactive | (deaf + muted)
        /// | Muted     | Inactive | (deaf + muted)
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
                    self.enable_voice_muted().await?;
                }
            }
            Ok(())
        }
    }

    /// Payload queued from the encoding loop to the network send task.
    struct AudioPacketOut {
        data: Vec<u8>,
        sequence: u64,
        is_terminator: bool,
    }

    /// Background task that reads from the microphone, encodes, and queues
    /// Opus packets for network transmission.
    ///
    /// Encoding and network I/O are decoupled via a bounded channel so
    /// that slow network sends never stall the audio processing loop.
    async fn outbound_audio_loop(mut pipeline: OutboundPipeline, handle: ClientHandle) {
        // Bounded channel: 50 packets ~ 1 second of audio at 20ms/frame.
        // If the network can't keep up, packets are dropped (preferable
        // to blocking the encoder and causing dropouts).
        let (tx, rx) = tokio::sync::mpsc::channel::<AudioPacketOut>(50);

        // Dedicated task for network sends - runs independently so
        // network latency never blocks encoding.
        tokio::spawn(outbound_send_task(rx, handle));

        // Let the capture buffer fill before we start encoding.
        // Without this, the first few ticks return NoData (dropout at t=0).
        tokio::time::sleep(Duration::from_millis(60)).await;

        // Poll at 5 ms instead of 20 ms.  Each frame is still 960 samples
        // (20 ms @ 48 kHz), but the shorter interval reduces the chance of
        // missing the moment when enough samples become available.
        // On Windows the default timer resolution is ~15.6 ms, so a 20 ms
        // interval can fire anywhere from 15.6-31.2 ms -- easily before
        // cpal has delivered a full frame, causing a dropout.  5 ms keeps
        // us well inside a single timer period and lets us catch frames
        // as soon as they appear.
        let mut interval = tokio::time::interval(Duration::from_millis(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut packet_count: u64 = 0;

        loop {
            interval.tick().await;

            // Process a bounded number of frames per tick. Under normal
            // conditions there is exactly 1 frame available (20ms of
            // captured audio). The bound prevents scheduler starvation
            // when catching up after a brief delay.
            for _ in 0..5 {
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
                        if tx
                            .try_send(AudioPacketOut {
                                data: packet.data,
                                sequence: packet.sequence,
                                is_terminator: false,
                            })
                            .is_err()
                        {
                            warn!("outbound_audio: send channel full, dropping packet");
                        }
                    }
                    Ok(OutboundTick::Terminator(packet)) => {
                        info!(
                            "outbound_audio: sending terminator (opus {} bytes)",
                            packet.data.len()
                        );
                        let _ = tx.try_send(AudioPacketOut {
                            data: packet.data,
                            sequence: packet.sequence,
                            is_terminator: true,
                        });
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

    /// Drains encoded audio packets from the channel and sends them to
    /// the server. Runs in its own tokio task so network latency never
    /// blocks the audio encoding loop.
    async fn outbound_send_task(
        mut rx: tokio::sync::mpsc::Receiver<AudioPacketOut>,
        handle: ClientHandle,
    ) {
        while let Some(pkt) = rx.recv().await {
            if let Err(e) = handle
                .send(command::SendAudio {
                    opus_data: pkt.data,
                    target: 0,
                    frame_number: pkt.sequence,
                    positional_data: None,
                    is_terminator: pkt.is_terminator,
                })
                .await
            {
                warn!("outbound_audio: network send failed: {e}");
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

    /// No-op on Android (no audio pipelines to restart).
    pub fn restart_outbound(&self) -> Result<(), String> {
        Ok(())
    }

    /// Audio is not yet supported on Android.
    pub async fn toggle_mute(&self) -> Result<(), String> {
        Err("Audio is not yet supported on Android".into())
    }

    /// Audio is not yet supported on Android.
    pub async fn toggle_deafen(&self) -> Result<(), String> {
        Err("Audio is not yet supported on Android".into())
    }
}
