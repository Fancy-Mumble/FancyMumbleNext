//! Audio / voice pipeline management: enable, disable, mute, deafen,
//! and the background outbound audio loop.
//!
//! Platform-specific audio I/O is abstracted behind factory functions
//! (`create_capture` / `create_playback`) that return trait objects.
//! On desktop, cpal is used; on Android, oboe.

use tauri::Emitter;

use super::types::VoiceState;
use super::AppState;

// -- Shared methods (all platforms) --------------------------------

impl AppState {
    /// Get current audio settings.
    pub fn audio_settings(&self) -> super::types::AudioSettings {
        self.inner.snapshot()
            .lock()
            .map(|s| s.audio.settings.clone())
            .unwrap_or_default()
    }

    /// Update audio settings.
    ///
    /// If any pipeline-relevant setting changed while voice is active,
    /// the outbound pipeline is automatically restarted.
    /// Volume changes are applied live via atomic handles (no restart).
    pub fn set_audio_settings(&self, settings: super::types::AudioSettings) -> Option<(bool, bool, bool)> {
        let (old_settings, voice_active) = {
            let __session = self.inner.snapshot();
            let state = __session.lock().ok()?;
            (
                state.audio.settings.clone(),
                state.audio.voice_state == VoiceState::Active,
            )
        };

        let restart_outbound = voice_active && old_settings.needs_pipeline_restart(&settings);
        let restart_inbound = voice_active && old_settings.needs_inbound_restart(&settings);
        let force_tcp_changed = old_settings.force_tcp_audio != settings.force_tcp_audio;

        // Update live volume handles (no pipeline restart needed).
        if let Ok(state) = self.inner.snapshot().lock() {
            use std::sync::atomic::Ordering;
            if let Some(ref h) = state.audio.input_volume_handle {
                h.store(settings.input_volume.to_bits(), Ordering::Relaxed);
            }
            if let Some(ref h) = state.audio.output_volume_handle {
                h.store(settings.output_volume.to_bits(), Ordering::Relaxed);
            }
        }

        if let Ok(mut state) = self.inner.snapshot().lock() {
            state.audio.settings = settings;
        }

        Some((restart_outbound, restart_inbound, force_tcp_changed))
    }

    /// Get current voice state.
    pub fn voice_state(&self) -> VoiceState {
        self.inner.snapshot()
            .lock()
            .map(|s| s.audio.voice_state)
            .unwrap_or_default()
    }

    /// Emit voice-state-changed event to the frontend.
    pub(super) fn emit_voice_state(&self) {
        if let Some(app) = self.app_handle() {
            let vs = self.voice_state();
            let _ = app.emit("voice-state-changed", vs);
        }
    }

    /// Set the local playback volume for a specific remote user.
    ///
    /// `volume` is a multiplier (0.0 = muted, 1.0 = normal, 2.0 = 200%).
    pub fn set_user_volume(&self, session: u32, volume: f32) {
        if let Ok(state) = self.inner.snapshot().lock() {
            if let Ok(mut sv) = state.audio.speaker_volumes.lock() {
                if (volume - 1.0).abs() < f32::EPSILON {
                    let _ = sv.remove(&session);
                } else {
                    let _ = sv.insert(session, volume.clamp(0.0, 2.0));
                }
            }
        }
    }
}

// -- Voice pipeline (all platforms) ---------------------------------

mod voice_pipeline {
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;
    use std::time::Duration;

    use tauri::Emitter;
    use tracing::info;

    use mumble_protocol::audio::encoder::{OpusEncoder, OpusEncoderConfig};
    use mumble_protocol::audio::filter::automatic_gain::{AgcConfig, AutomaticGainControl};
    use mumble_protocol::audio::filter::denoiser::{DenoiserConfig, SpectralDenoiser};
    use mumble_protocol::audio::filter::noise_gate::{NoiseGate, NoiseGateConfig};
    use mumble_protocol::audio::filter::FilterChain;
    use mumble_protocol::audio::mixer::{AudioMixer, SpeakerBuffers};
    use mumble_protocol::audio::pipeline::OutboundPipeline;
    use mumble_protocol::audio::sample::AudioFormat;
    use mumble_protocol::client::ClientHandle;
    use mumble_protocol::command;

    use crate::audio::{AudioDeviceFactory, PlatformAudioFactory};

    use crate::state::types::{AudioSettings, VoiceState};
    use crate::state::AppState;

    impl AppState {
        /// Enable voice calling: unmute + undeaf, start audio pipelines.
        pub async fn enable_voice(&self) -> Result<(), String> {
            let (handle, audio_settings) = {
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                (state.conn.client_handle.clone(), state.audio.settings.clone())
            };

            info!("enable_voice: starting audio pipelines");

            // Create shared volume handles for live updates.
            let input_vol = Arc::new(AtomicU32::new(audio_settings.input_volume.to_bits()));
            let output_vol = Arc::new(AtomicU32::new(audio_settings.output_volume.to_bits()));

            // Inbound: per-speaker decoders + mixing playback.
            let speaker_buffers: SpeakerBuffers = Arc::new(
                std::sync::Mutex::new(std::collections::HashMap::new()),
            );
            let speaker_volumes = {
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                state.audio.speaker_volumes.clone()
            };
            let mixer = AudioMixer::new(speaker_buffers.clone(), AudioFormat::MONO_48KHZ_F32);
            let mut mixing_playback = PlatformAudioFactory::create_mixing_playback(
                audio_settings.selected_output_device.as_deref(),
                output_vol.clone(),
                speaker_buffers,
                speaker_volumes,
            )?;
            mixing_playback
                .start()
                .map_err(|e| format!("Playback start: {e}"))?;

            {
                let __session = self.inner.snapshot();
                let mut state = __session.lock().map_err(|e| e.to_string())?;
                state.audio.mixer = Some(mixer);
                state.audio.mixing_playback = Some(mixing_playback);
                state.audio.input_volume_handle = Some(input_vol);
                state.audio.output_volume_handle = Some(output_vol);
            }

            // Outbound: capture -> filters -> encoder -> network.
            self.start_outbound_pipeline(&audio_settings, &handle)?;

            {
                let __session = self.inner.snapshot();
                let mut state = __session.lock().map_err(|e| e.to_string())?;
                state.audio.voice_state = VoiceState::Active;
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
                let __session = self.inner.snapshot();
                let mut state = __session.lock().map_err(|e| e.to_string())?;
                state.audio.voice_state = VoiceState::Inactive;
                state.conn.client_handle.clone()
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
            self.stop_audio_on(&self.inner.snapshot());
        }

        /// Stop all running audio pipelines and tasks for a specific
        /// session arc.  Used during multi-server active-session swaps
        /// so the previous session's pipeline can be torn down even
        /// after `inner` has already been swapped to a new session.
        pub(in crate::state) fn stop_audio_on(
            &self,
            arc: &Arc<std::sync::Mutex<crate::state::SharedState>>,
        ) {
            let stopped_sessions: Vec<(u32, tauri::AppHandle)> = if let Ok(mut state) = arc.lock() {
                if let Some(handle) = state.audio.outbound_task_handle.take() {
                    handle.abort();
                }
                if let Some(handle) = state.audio.mic_test_handle.take() {
                    handle.abort();
                }
                if let Some(handle) = state.audio.latency_test_handle.take() {
                    handle.abort();
                }
                if let Some(mut playback) = state.audio.mixing_playback.take() {
                    let _ = playback.stop();
                }
                state.audio.mixer = None;
                state.audio.input_volume_handle = None;
                state.audio.output_volume_handle = None;

                // Collect sessions to notify OUTSIDE the lock.
                let sessions: Vec<(u32, tauri::AppHandle)> = state
                    .conn.tauri_app_handle
                    .as_ref()
                    .filter(|_| !state.audio.talking_sessions.is_empty())
                    .map(|app| state.audio.talking_sessions.iter().map(|&s| (s, app.clone())).collect())
                    .unwrap_or_default();
                state.audio.talking_sessions.clear();
                sessions
            } else {
                Vec::new()
            };

            // Emit outside the lock to avoid deadlock with Tauri IPC.
            for (session, app) in stopped_sessions {
                let _ = app.emit("user-talking", (session, false));
            }
        }

        /// Stop only the outbound (mic capture) pipeline.
        fn stop_outbound(&self) {
            if let Ok(mut state) = self.inner.snapshot().lock() {
                if let Some(handle) = state.audio.outbound_task_handle.take() {
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
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                (state.audio.settings.clone(), state.conn.client_handle.clone())
            };

            self.start_outbound_pipeline(&audio_settings, &client_handle)
        }

        /// Restart the inbound pipeline with the current audio settings.
        ///
        /// Called when the output device changes while voice is active.
        pub fn restart_inbound(&self) -> Result<(), String> {
            info!("restart_inbound: restarting playback pipeline with new settings");

            // Stop the old inbound pipeline.
            if let Ok(mut state) = self.inner.snapshot().lock() {
                if let Some(mut playback) = state.audio.mixing_playback.take() {
                    let _ = playback.stop();
                }
                state.audio.mixer = None;
            }

            let audio_settings = {
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                state.audio.settings.clone()
            };

            let output_vol = Arc::new(AtomicU32::new(audio_settings.output_volume.to_bits()));

            let speaker_buffers: SpeakerBuffers = Arc::new(
                std::sync::Mutex::new(std::collections::HashMap::new()),
            );
            let speaker_volumes = {
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                state.audio.speaker_volumes.clone()
            };
            let mixer = AudioMixer::new(speaker_buffers.clone(), AudioFormat::MONO_48KHZ_F32);
            let mut mixing_playback = PlatformAudioFactory::create_mixing_playback(
                audio_settings.selected_output_device.as_deref(),
                output_vol.clone(),
                speaker_buffers,
                speaker_volumes,
            )?;
            mixing_playback
                .start()
                .map_err(|e| format!("Playback start: {e}"))?;

            let __session = self.inner.snapshot();

            let mut state = __session.lock().map_err(|e| e.to_string())?;
            state.audio.mixer = Some(mixer);
            state.audio.mixing_playback = Some(mixing_playback);
            state.audio.output_volume_handle = Some(output_vol);
            Ok(())
        }

        /// Create and start a fresh outbound audio pipeline (mic -> encoder -> network).
        fn start_outbound_pipeline(
            &self,
            audio_settings: &AudioSettings,
            client_handle: &Option<ClientHandle>,
        ) -> Result<(), String> {
            // Re-use existing input volume handle or create a new one.
            let (input_vol, app, own_session) = {
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                (
                    state.audio.input_volume_handle.clone(),
                    state.conn.tauri_app_handle.clone(),
                    state.conn.own_session,
                )
            };
            let input_vol =
                input_vol.unwrap_or_else(|| Arc::new(AtomicU32::new(audio_settings.input_volume.to_bits())));

            let capture = PlatformAudioFactory::create_capture(
                audio_settings.selected_device.as_deref(),
                audio_settings.frame_size_samples(),
                input_vol.clone(),
            )?;

            let encoder_config = OpusEncoderConfig {
                bitrate: audio_settings.bitrate_bps,
                frame_size: audio_settings.frame_size_samples(),
                ..OpusEncoderConfig::default()
            };
            let encoder = OpusEncoder::new(encoder_config, AudioFormat::MONO_48KHZ_F32)
                .map_err(|e| format!("Encoder init: {e}"))?;

            let mut outbound_filters = FilterChain::new();
            // AGC before noise gate (see enable_voice for rationale).
            if audio_settings.auto_gain {
                let max_gain_linear = 10.0_f32.powf(audio_settings.max_gain_db / 20.0);
                #[cfg(target_os = "android")]
                let agc_config = AgcConfig {
                    max_gain: max_gain_linear.min(2.0),
                    ..AgcConfig::default()
                };
                #[cfg(not(target_os = "android"))]
                let agc_config = AgcConfig {
                    max_gain: max_gain_linear,
                    ..AgcConfig::default()
                };
                outbound_filters.push(Box::new(AutomaticGainControl::new(agc_config)));
            }
            if audio_settings.noise_suppression {
                // RNN-based denoiser runs first so the gate sees clean
                // audio and does not chatter on transient noise.
                let denoiser_config = DenoiserConfig {
                    algorithm: audio_settings.denoiser_algorithm,
                    params: audio_settings.denoiser_params.clone(),
                    ..DenoiserConfig::default()
                };
                outbound_filters.push(Box::new(SpectralDenoiser::new(denoiser_config)));
                outbound_filters.push(Box::new(NoiseGate::new(NoiseGateConfig {
                    open_threshold: audio_settings.vad_threshold,
                    close_threshold: audio_settings.vad_threshold * audio_settings.noise_gate_close_ratio,
                    hold_frames: audio_settings.hold_frames,
                    ..NoiseGateConfig::default()
                })));
            }

            info!(
                "start_outbound_pipeline: filters={}, noise_suppression={}, threshold={:.5}",
                outbound_filters.len(),
                audio_settings.noise_suppression,
                audio_settings.vad_threshold,
            );

            let outbound = OutboundPipeline::new(
                capture,
                outbound_filters,
                Box::new(encoder),
            );
            // capture.start() is deferred to outbound_audio_loop so the
            // cpal stream only begins producing samples once the encoding
            // loop is ready to consume them, avoiding buffer overflow on
            // startup.

            let outbound_handle = if let Some(ref client) = client_handle {
                let client = client.clone();
                let app = app.clone();
                Some(tokio::spawn(async move {
                    outbound_audio_loop(outbound, client, app, own_session).await;
                }))
            } else {
                None
            };

            let __session = self.inner.snapshot();

            let mut state = __session.lock().map_err(|e| e.to_string())?;
            state.audio.outbound_task_handle = outbound_handle;
            state.audio.input_volume_handle = Some(input_vol);
            Ok(())
        }

        /// Enable voice in muted state: start inbound pipeline (hearing)
        /// but keep outbound stopped (mic off).
        ///
        /// Used when undeafening from Inactive to land in Muted state.
        pub async fn enable_voice_muted(&self) -> Result<(), String> {
            let (handle, audio_settings) = {
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                (state.conn.client_handle.clone(), state.audio.settings.clone())
            };

            info!("enable_voice_muted: starting inbound pipeline only");

            let output_vol = Arc::new(AtomicU32::new(audio_settings.output_volume.to_bits()));

            let speaker_buffers: SpeakerBuffers = Arc::new(
                std::sync::Mutex::new(std::collections::HashMap::new()),
            );
            let speaker_volumes = {
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                state.audio.speaker_volumes.clone()
            };
            let mixer = AudioMixer::new(speaker_buffers.clone(), AudioFormat::MONO_48KHZ_F32);
            let mut mixing_playback = PlatformAudioFactory::create_mixing_playback(
                audio_settings.selected_output_device.as_deref(),
                output_vol.clone(),
                speaker_buffers,
                speaker_volumes,
            )?;
            mixing_playback
                .start()
                .map_err(|e| format!("Playback start: {e}"))?;

            {
                let __session = self.inner.snapshot();
                let mut state = __session.lock().map_err(|e| e.to_string())?;
                state.audio.voice_state = VoiceState::Muted;
                state.audio.mixer = Some(mixer);
                state.audio.mixing_playback = Some(mixing_playback);
                state.audio.output_volume_handle = Some(output_vol);
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
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                (
                    state.audio.voice_state,
                    state.conn.client_handle.clone(),
                    state.audio.settings.clone(),
                )
            };

            match voice_state {
                VoiceState::Active => {
                    info!("toggle_mute: muting (stopping outbound)");
                    self.stop_outbound();
                    {
                        let __session = self.inner.snapshot();
                        let mut state = __session.lock().map_err(|e| e.to_string())?;
                        state.audio.voice_state = VoiceState::Muted;
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
                        let __session = self.inner.snapshot();
                        let mut state = __session.lock().map_err(|e| e.to_string())?;
                        state.audio.voice_state = VoiceState::Active;
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
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                state.audio.voice_state
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

        /// Start the mic test: opens a capture stream and emits
        /// `mic-amplitude` events to the frontend at ~30 Hz.
        ///
        /// When `auto_input_sensitivity` is enabled, the measured
        /// noise floor (post-AGC) is used to auto-adjust `vad_threshold`.
        pub fn start_mic_test(&self) -> Result<(), String> {
            // Stop any already-running mic test.
            self.stop_mic_test();

            let audio_settings = {
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                state.audio.settings.clone()
            };

            let input_vol = Arc::new(AtomicU32::new(audio_settings.input_volume.to_bits()));

            let mut capture = PlatformAudioFactory::create_capture(
                audio_settings.selected_device.as_deref(),
                960, // 20ms @ 48kHz
                input_vol,
            )?;

            capture.start().map_err(|e| format!("Mic test capture start: {e}"))?;

            // Build AGC filter matching the voice pipeline so auto-calibration
            // measures post-gain levels (same as the noise gate will see).
            let agc_filter = if audio_settings.auto_gain {
                let max_gain_linear = 10.0_f32.powf(audio_settings.max_gain_db / 20.0);
                #[cfg(target_os = "android")]
                let agc_config = AgcConfig {
                    max_gain: max_gain_linear.min(2.0),
                    ..AgcConfig::default()
                };
                #[cfg(not(target_os = "android"))]
                let agc_config = AgcConfig {
                    max_gain: max_gain_linear,
                    ..AgcConfig::default()
                };
                Some(AutomaticGainControl::new(agc_config))
            } else {
                None
            };

            let app = self.app_handle().ok_or("No app handle")?;
            let auto_sensitivity = audio_settings.auto_input_sensitivity;
            let inner = self.inner.clone_arc();

            let handle = tauri::async_runtime::spawn(async move {
                mic_test_loop(capture, app, auto_sensitivity, inner, agc_filter).await;
            });

            let __session = self.inner.snapshot();

            let mut state = __session.lock().map_err(|e| e.to_string())?;
            state.audio.mic_test_handle = Some(handle);
            Ok(())
        }

        /// Stop the mic test.
        pub fn stop_mic_test(&self) {
            if let Ok(mut state) = self.inner.snapshot().lock() {
                if let Some(handle) = state.audio.mic_test_handle.take() {
                    handle.abort();
                }
            }
        }

        /// Start sending TCP pings at high frequency to measure round-trip latency.
        ///
        /// RTT is computed in the event handler when the server echoes the ping
        /// back, and emitted as a `"ping-latency"` Tauri event.
        pub fn start_latency_test(&self) -> Result<(), String> {
            self.stop_latency_test();

            let client_handle = {
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                state
                    .conn.client_handle
                    .clone()
                    .ok_or_else(|| "Not connected".to_string())?
            };

            let handle = tauri::async_runtime::spawn(async move {
                latency_ping_loop(client_handle).await;
            });

            let __session = self.inner.snapshot();

            let mut state = __session.lock().map_err(|e| e.to_string())?;
            state.audio.latency_test_handle = Some(handle);
            Ok(())
        }

        /// Stop the latency test.
        pub fn stop_latency_test(&self) {
            if let Ok(mut state) = self.inner.snapshot().lock() {
                if let Some(handle) = state.audio.latency_test_handle.take() {
                    handle.abort();
                }
            }
        }

        /// Calibrate the voice activation threshold.
        ///
        /// Opens the microphone, applies the same AGC filter chain as
        /// the voice pipeline, measures the post-gain noise floor over
        /// ~2 seconds, and sets `vad_threshold` to 3x the measured
        /// noise floor.  Returns the new threshold.
        pub async fn calibrate_voice_threshold(&self) -> Result<f32, String> {
            let audio_settings = {
                let __session = self.inner.snapshot();
                let state = __session.lock().map_err(|e| e.to_string())?;
                state.audio.settings.clone()
            };

            let input_vol = Arc::new(AtomicU32::new(audio_settings.input_volume.to_bits()));

            let mut capture = PlatformAudioFactory::create_capture(
                audio_settings.selected_device.as_deref(),
                960,
                input_vol,
            )?;
            capture.start().map_err(|e| format!("Calibration capture start: {e}"))?;

            // Build the same AGC filter as the voice pipeline.
            let mut agc_filter = if audio_settings.auto_gain {
                let max_gain_linear = 10.0_f32.powf(audio_settings.max_gain_db / 20.0);
                #[cfg(target_os = "android")]
                let agc_config = AgcConfig {
                    max_gain: max_gain_linear.min(2.0),
                    ..AgcConfig::default()
                };
                #[cfg(not(target_os = "android"))]
                let agc_config = AgcConfig {
                    max_gain: max_gain_linear,
                    ..AgcConfig::default()
                };
                Some(AutomaticGainControl::new(agc_config))
            } else {
                None
            };

            // Measure noise floor for ~2 seconds (~60 frames at 30 Hz).
            let mut noise_floor_ema: f32 = 0.0;
            let ema_alpha: f32 = 0.1;
            let mut frame_count: u32 = 0;

            let mut interval = tokio::time::interval(Duration::from_millis(33));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            for _ in 0..60 {
                let _ = interval.tick().await;

                let mut latest = None;
                while let Ok(frame) = capture.read_frame() {
                    latest = Some(frame);
                }
                let Some(mut frame) = latest else {
                    continue;
                };

                // Apply AGC to get post-gain levels.
                if let Some(ref mut agc) = agc_filter {
                    use mumble_protocol::audio::filter::AudioFilter as _;
                    let _ = agc.process(&mut frame);
                }

                let samples: Vec<f32> = frame
                    .data
                    .chunks_exact(4)
                    .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();

                if samples.is_empty() {
                    continue;
                }

                let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
                let rms = (sum_sq / samples.len() as f32).sqrt().min(1.0);

                frame_count += 1;
                if noise_floor_ema < 0.0001 {
                    noise_floor_ema = rms;
                } else {
                    // Only update from frames close to the noise floor.
                    // Use a tight 3x multiplier so speech does not
                    // inflate the noise floor estimate.
                    let is_quiet = frame_count < 15
                        || rms < noise_floor_ema * 3.0
                        || noise_floor_ema < 0.0001;
                    noise_floor_ema = apply_ema_if_quiet(noise_floor_ema, rms, ema_alpha, is_quiet);
                }
            }

            let _ = capture.stop();

            // Set threshold at 15x noise floor for clear separation.
            let threshold = (noise_floor_ema * 15.0).clamp(0.03, 0.5);
            info!(
                "calibrate_voice_threshold: noise_floor={:.5}, threshold={:.5}",
                noise_floor_ema, threshold
            );

            if let Ok(mut state) = self.inner.snapshot().lock() {
                state.audio.settings.vad_threshold = threshold;
            }

            // Emit event so the frontend can update the displayed threshold.
            if let Some(app) = self.app_handle() {
                use tauri::Emitter as _;
                let _ = app.emit("vad-threshold-updated", threshold);
            }

            Ok(threshold)
        }
    }

    fn apply_ema_if_quiet(current_ema: f32, rms: f32, alpha: f32, is_quiet: bool) -> f32 {
        if is_quiet {
            alpha * rms + (1.0 - alpha) * current_ema
        } else {
            current_ema
        }
    }

    // Background task functions are in the `audio_tasks` sibling module
    // to keep this file's size manageable.
    use super::super::audio_tasks::{latency_ping_loop, mic_test_loop, outbound_audio_loop};

    #[cfg(test)]
    mod tests {
        use super::*;
        use mumble_protocol::audio::sample::AudioFormat;

        #[test]
        #[ignore = "requires audio hardware - run with --ignored"]
        fn create_capture_returns_correct_format() {
            let vol = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
            let Ok(capture) = PlatformAudioFactory::create_capture(None, 960, vol) else {
                eprintln!("Skipping: no audio input device available");
                return;
            };
            assert_eq!(capture.format(), AudioFormat::MONO_48KHZ_F32);
        }

        #[test]
        #[ignore = "requires audio hardware - run with --ignored"]
        fn factory_capture_is_compatible_with_pipeline() {
            use mumble_protocol::audio::filter::FilterChain;
            use mumble_protocol::audio::pipeline::OutboundPipeline;

            let vol = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
            let Ok(capture) = PlatformAudioFactory::create_capture(None, 960, vol) else { return };

            let Ok(encoder) = OpusEncoder::new(
                OpusEncoderConfig::default(),
                AudioFormat::MONO_48KHZ_F32,
            ) else {
                return;
            };

            let _pipeline = OutboundPipeline::new(
                capture,
                FilterChain::new(),
                Box::new(encoder),
            );
        }

        #[test]
        #[ignore = "requires audio hardware - run with --ignored"]
        fn factory_mixing_playback_can_be_started() {
            let vol = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
            let bufs: SpeakerBuffers = Arc::new(
                std::sync::Mutex::new(std::collections::HashMap::new()),
            );
            let sv: mumble_protocol::audio::mixer::SpeakerVolumes = Default::default();
            let Ok(mut playback) = PlatformAudioFactory::create_mixing_playback(None, vol, bufs, sv)
            else {
                eprintln!("Skipping: no audio output device available");
                return;
            };
            if playback.start().is_err() {
                eprintln!("Skipping: audio output device found but stream could not start");
                return;
            }
            assert!(playback.stop().is_ok());
        }
    }
}
