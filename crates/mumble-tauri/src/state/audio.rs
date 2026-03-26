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
    pub(super) fn emit_voice_state(&self) {
        if let Some(app) = self.app_handle() {
            let vs = self.voice_state();
            let _ = app.emit("voice-state-changed", vs);
        }
    }
}

// -- Voice pipeline (all platforms) ---------------------------------

mod voice_pipeline {
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;
    use std::time::Duration;

    use tracing::{info, warn};

    use mumble_protocol::audio::encoder::{OpusEncoder, OpusEncoderConfig};
    use mumble_protocol::audio::filter::automatic_gain::{AgcConfig, AutomaticGainControl};
    use mumble_protocol::audio::filter::noise_gate::{NoiseGate, NoiseGateConfig};
    use mumble_protocol::audio::filter::FilterChain;
    use mumble_protocol::audio::mixer::{AudioMixer, SpeakerBuffers};
    use mumble_protocol::audio::pipeline::{OutboundPipeline, OutboundTick};
    use mumble_protocol::audio::sample::AudioFormat;
    use mumble_protocol::client::ClientHandle;
    use mumble_protocol::command;

    use crate::audio::{AudioDeviceFactory, PlatformAudioFactory};
    use mumble_protocol::audio::capture::AudioCapture;

    use crate::state::types::{AudioSettings, MicAmplitudePayload, VoiceState};
    use crate::state::{AppState, SharedState};

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

            // Inbound: per-speaker decoders + mixing playback.
            let speaker_buffers: SpeakerBuffers = Arc::new(
                std::sync::Mutex::new(std::collections::HashMap::new()),
            );
            let mixer = AudioMixer::new(speaker_buffers.clone(), AudioFormat::MONO_48KHZ_F32);
            let mut mixing_playback = PlatformAudioFactory::create_mixing_playback(
                audio_settings.selected_output_device.as_deref(),
                output_vol.clone(),
                speaker_buffers,
            )?;
            mixing_playback
                .start()
                .map_err(|e| format!("Playback start: {e}"))?;

            // Outbound: capture -> filters -> encoder -> network.
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
            // AGC runs before the noise gate so the gate evaluates
            // the post-gain signal level.
            //
            // On Android the VoiceRecognition input preset applies
            // platform-level AGC (but no noise suppression / AEC),
            // so we cap our AGC at 6 dB (2x) to gently normalise
            // without over-amplifying the already-gained signal.
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
                let noise_gate = NoiseGate::new(NoiseGateConfig {
                    open_threshold: audio_settings.vad_threshold,
                    close_threshold: audio_settings.vad_threshold * audio_settings.noise_gate_close_ratio,
                    hold_frames: audio_settings.hold_frames,
                    ..NoiseGateConfig::default()
                });
                outbound_filters.push(Box::new(noise_gate));
            }

            let mut outbound = OutboundPipeline::new(
                capture,
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
                state.audio_mixer = Some(mixer);
                state.mixing_playback = Some(mixing_playback);
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
                if let Some(handle) = state.mic_test_handle.take() {
                    handle.abort();
                }
                if let Some(handle) = state.latency_test_handle.take() {
                    handle.abort();
                }
                if let Some(mut playback) = state.mixing_playback.take() {
                    let _ = playback.stop();
                }
                state.audio_mixer = None;
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
                if let Some(mut playback) = state.mixing_playback.take() {
                    let _ = playback.stop();
                }
                state.audio_mixer = None;
            }

            let audio_settings = {
                let state = self.inner.lock().map_err(|e| e.to_string())?;
                state.audio_settings.clone()
            };

            let output_vol = Arc::new(AtomicU32::new(audio_settings.output_volume.to_bits()));

            let speaker_buffers: SpeakerBuffers = Arc::new(
                std::sync::Mutex::new(std::collections::HashMap::new()),
            );
            let mixer = AudioMixer::new(speaker_buffers.clone(), AudioFormat::MONO_48KHZ_F32);
            let mut mixing_playback = PlatformAudioFactory::create_mixing_playback(
                audio_settings.selected_output_device.as_deref(),
                output_vol.clone(),
                speaker_buffers,
            )?;
            mixing_playback
                .start()
                .map_err(|e| format!("Playback start: {e}"))?;

            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.audio_mixer = Some(mixer);
            state.mixing_playback = Some(mixing_playback);
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
                outbound_filters.push(Box::new(NoiseGate::new(NoiseGateConfig {
                    open_threshold: audio_settings.vad_threshold,
                    close_threshold: audio_settings.vad_threshold * audio_settings.noise_gate_close_ratio,
                    hold_frames: audio_settings.hold_frames,
                    ..NoiseGateConfig::default()
                })));
            }

            let mut outbound = OutboundPipeline::new(
                capture,
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

            let speaker_buffers: SpeakerBuffers = Arc::new(
                std::sync::Mutex::new(std::collections::HashMap::new()),
            );
            let mixer = AudioMixer::new(speaker_buffers.clone(), AudioFormat::MONO_48KHZ_F32);
            let mut mixing_playback = PlatformAudioFactory::create_mixing_playback(
                audio_settings.selected_output_device.as_deref(),
                output_vol.clone(),
                speaker_buffers,
            )?;
            mixing_playback
                .start()
                .map_err(|e| format!("Playback start: {e}"))?;

            {
                let mut state = self.inner.lock().map_err(|e| e.to_string())?;
                state.voice_state = VoiceState::Muted;
                state.audio_mixer = Some(mixer);
                state.mixing_playback = Some(mixing_playback);
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

        /// Start the mic test: opens a capture stream and emits
        /// `mic-amplitude` events to the frontend at ~30 Hz.
        ///
        /// When `auto_input_sensitivity` is enabled, the measured
        /// noise floor is used to auto-adjust `vad_threshold`.
        pub fn start_mic_test(&self) -> Result<(), String> {
            // Stop any already-running mic test.
            self.stop_mic_test();

            let audio_settings = {
                let state = self.inner.lock().map_err(|e| e.to_string())?;
                state.audio_settings.clone()
            };

            let input_vol = Arc::new(AtomicU32::new(audio_settings.input_volume.to_bits()));

            let mut capture = PlatformAudioFactory::create_capture(
                audio_settings.selected_device.as_deref(),
                960, // 20ms @ 48kHz
                input_vol,
            )?;

            capture.start().map_err(|e| format!("Mic test capture start: {e}"))?;

            let app = self.app_handle().ok_or("No app handle")?;
            let auto_sensitivity = audio_settings.auto_input_sensitivity;
            let inner = self.inner.clone();

            let handle = tauri::async_runtime::spawn(async move {
                mic_test_loop(capture, app, auto_sensitivity, inner).await;
            });

            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.mic_test_handle = Some(handle);
            Ok(())
        }

        /// Stop the mic test.
        pub fn stop_mic_test(&self) {
            if let Ok(mut state) = self.inner.lock() {
                if let Some(handle) = state.mic_test_handle.take() {
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
                let state = self.inner.lock().map_err(|e| e.to_string())?;
                state
                    .client_handle
                    .clone()
                    .ok_or_else(|| "Not connected".to_string())?
            };

            let handle = tauri::async_runtime::spawn(async move {
                latency_ping_loop(client_handle).await;
            });

            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.latency_test_handle = Some(handle);
            Ok(())
        }

        /// Stop the latency test.
        pub fn stop_latency_test(&self) {
            if let Ok(mut state) = self.inner.lock() {
                if let Some(handle) = state.latency_test_handle.take() {
                    handle.abort();
                }
            }
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
        let _outbound_send_task = tokio::spawn(outbound_send_task(rx, handle));

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
            let _ = interval.tick().await;

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

    /// Background loop for the mic test.
    ///
    /// Reads frames from the capture device, computes RMS/peak, and
    /// emits `mic-amplitude` events to the frontend.  When
    /// `auto_sensitivity` is enabled, measures the noise floor over a
    /// sliding window and writes the auto-computed `vad_threshold`
    /// back into `AudioSettings`.
    async fn mic_test_loop(
        mut capture: Box<dyn AudioCapture>,
        app: tauri::AppHandle,
        auto_sensitivity: bool,
        inner: Arc<std::sync::Mutex<SharedState>>,
    ) {
        use tauri::Emitter;

        // Emit at ~30 Hz (every ~33 ms).
        let mut interval = tokio::time::interval(Duration::from_millis(33));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        // Exponential moving average of the noise floor for auto-sensitivity.
        let mut noise_floor_ema: f32 = 0.0;
        let ema_alpha: f32 = 0.05; // slow adaptation
        let mut frame_count: u64 = 0;

        loop {
            let _ = interval.tick().await;

            // Drain ALL buffered frames so we always measure the most
            // recent audio.  Without this the ring buffer fills up
            // (cpal produces ~50 fps, we read at ~30 fps) causing
            // ever-growing latency and eventual overflow warnings.
            let mut latest = None;
            while let Ok(frame) = capture.read_frame() {
                latest = Some(frame);
            }
            let Some(frame) = latest else {
                continue;
            };

            // Parse f32 samples from the raw byte data.
            let samples: Vec<f32> = frame
                .data
                .chunks_exact(4)
                .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
                .collect();

            if samples.is_empty() {
                continue;
            }

            let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
            let rms = (sum_sq / samples.len() as f32).sqrt();
            let peak = samples.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);

            // Clamp to [0, 1].
            let rms = rms.min(1.0);
            let peak = peak.min(1.0);

            let _ = app.emit("mic-amplitude", MicAmplitudePayload { rms, peak });

            // Auto-sensitivity: adapt noise floor and set threshold.
            if auto_sensitivity {
                frame_count += 1;

                // Only update noise floor from quiet frames (likely ambient noise).
                // A frame is considered "quiet" if its RMS < 3x the current noise floor
                // estimate, or during the initial calibration period.
                let is_calibrating = frame_count < 30; // ~1 second warmup
                let is_quiet = rms < noise_floor_ema * 3.0 || noise_floor_ema < 0.0001;

                if is_calibrating || is_quiet {
                    if noise_floor_ema < 0.0001 {
                        noise_floor_ema = rms;
                    } else {
                        noise_floor_ema = ema_alpha * rms + (1.0 - ema_alpha) * noise_floor_ema;
                    }
                }

                // Set threshold at 2x the noise floor, with a minimum.
                // This gives headroom above ambient noise but catches speech.
                if frame_count > 15 && frame_count.is_multiple_of(10) {
                    let threshold = (noise_floor_ema * 2.0).clamp(0.005, 0.5);
                    if let Ok(mut state) = inner.lock() {
                        if (state.audio_settings.vad_threshold - threshold).abs() > 0.002 {
                            state.audio_settings.vad_threshold = threshold;
                        }
                    }
                }
            }
        }
    }

    /// Background task that sends TCP pings at ~2 Hz so the event handler can
    /// compute RTT and emit `"ping-latency"` events for the latency graph.
    async fn latency_ping_loop(client_handle: ClientHandle) {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            let _ = interval.tick().await;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if client_handle
                .send(command::SendPing { timestamp: ts })
                .await
                .is_err()
            {
                break;
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use mumble_protocol::audio::sample::AudioFormat;

        #[test]
        fn create_capture_returns_correct_format() {
            let vol = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
            let Ok(capture) = PlatformAudioFactory::create_capture(None, 960, vol) else {
                eprintln!("Skipping: no audio input device available");
                return;
            };
            assert_eq!(capture.format(), AudioFormat::MONO_48KHZ_F32);
        }

        #[test]
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
        fn factory_mixing_playback_can_be_started() {
            let vol = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
            let bufs: SpeakerBuffers = Arc::new(
                std::sync::Mutex::new(std::collections::HashMap::new()),
            );
            let Ok(mut playback) = PlatformAudioFactory::create_mixing_playback(None, vol, bufs)
            else {
                eprintln!("Skipping: no audio output device available");
                return;
            };
            assert!(playback.start().is_ok());
            assert!(playback.stop().is_ok());
        }
    }
}
