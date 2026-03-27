//! Background async tasks for audio: outbound encoding/sending loop,
//! mic test loop with auto-calibration, and latency ping loop.
//!
//! Extracted from `audio.rs` to keep file sizes manageable.

use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use mumble_protocol::audio::capture::AudioCapture;
use mumble_protocol::audio::filter::automatic_gain::AutomaticGainControl;
use mumble_protocol::audio::pipeline::{OutboundPipeline, OutboundTick};
use mumble_protocol::client::ClientHandle;
use mumble_protocol::command;

use super::types::MicAmplitudePayload;
use super::SharedState;

// -----------------------------------------------------------------------
//  Outbound audio
// -----------------------------------------------------------------------

/// Payload queued from the encoding loop to the network send task.
pub(super) struct AudioPacketOut {
    pub data: Vec<u8>,
    pub sequence: u64,
    pub is_terminator: bool,
}

/// Background task that reads from the microphone, encodes, and queues
/// Opus packets for network transmission.
///
/// Encoding and network I/O are decoupled via a bounded channel so
/// that slow network sends never stall the audio processing loop.
pub(super) async fn outbound_audio_loop(mut pipeline: OutboundPipeline, handle: ClientHandle) {
    // Bounded channel: 50 packets ~ 1 second of audio at 20ms/frame.
    let (tx, rx) = tokio::sync::mpsc::channel::<AudioPacketOut>(50);

    let _outbound_send_task = tokio::spawn(outbound_send_task(rx, handle));

    // Let the capture buffer fill before we start encoding.
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Poll at 5 ms instead of 20 ms. Each frame is still 960 samples
    // (20 ms @ 48 kHz), but the shorter interval reduces the chance of
    // missing the moment when enough samples become available.
    let mut interval = tokio::time::interval(Duration::from_millis(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut packet_count: u64 = 0;
    let mut silence_count: u64 = 0;
    let mut total_ticks: u64 = 0;

    loop {
        let _ = interval.tick().await;

        // Process a bounded number of frames per tick.
        for _ in 0..5 {
            match pipeline.tick() {
                Ok(OutboundTick::Audio(packet)) => {
                    packet_count += 1;
                    total_ticks += 1;
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
                    total_ticks += 1;
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
                    silence_count += 1;
                    total_ticks += 1;
                    // Periodic diagnostic: every ~10 seconds at 50 fps.
                    if total_ticks.is_multiple_of(500) {
                        info!(
                            "outbound_audio stats: total={}, sent={}, silenced={}, silence_rate={:.1}%",
                            total_ticks,
                            packet_count,
                            silence_count,
                            if total_ticks > 0 {
                                silence_count as f64 / total_ticks as f64 * 100.0
                            } else {
                                0.0
                            },
                        );
                    }
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
/// the server.
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

// -----------------------------------------------------------------------
//  Mic test
// -----------------------------------------------------------------------

/// Background loop for the mic test.
///
/// Reads frames from the capture device, computes RMS/peak, and
/// emits `mic-amplitude` events to the frontend.  When
/// `auto_sensitivity` is enabled, applies AGC to measure
/// post-gain levels for noise floor estimation and writes the
/// auto-computed `vad_threshold` back into `AudioSettings`.
pub(super) async fn mic_test_loop(
    mut capture: Box<dyn AudioCapture>,
    app: tauri::AppHandle,
    auto_sensitivity: bool,
    inner: Arc<std::sync::Mutex<SharedState>>,
    mut agc_filter: Option<AutomaticGainControl>,
) {
    use mumble_protocol::audio::filter::AudioFilter as _;
    use tauri::Emitter;

    let mut interval = tokio::time::interval(Duration::from_millis(33));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut noise_floor_ema: f32 = 0.0;
    let ema_alpha: f32 = 0.05;
    let mut frame_count: u64 = 0;

    loop {
        let _ = interval.tick().await;

        // Drain all buffered frames to avoid latency buildup.
        let mut latest = None;
        while let Ok(frame) = capture.read_frame() {
            latest = Some(frame);
        }
        let Some(frame) = latest else {
            continue;
        };

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
        let peak = samples
            .iter()
            .map(|s| s.abs())
            .fold(0.0_f32, f32::max)
            .min(1.0);

        let _ = app.emit("mic-amplitude", MicAmplitudePayload { rms, peak });

        // Auto-sensitivity: adapt noise floor and set threshold.
        // Apply AGC to get the same signal level the noise gate
        // would see in the voice pipeline.
        if auto_sensitivity {
            frame_count += 1;

            let calibration_rms = if let Some(ref mut agc) = agc_filter {
                let mut agc_frame = frame;
                let _ = agc.process(&mut agc_frame);
                let agc_samples: Vec<f32> = agc_frame
                    .data
                    .chunks_exact(4)
                    .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                let sq: f32 = agc_samples.iter().map(|s| s * s).sum();
                (sq / agc_samples.len() as f32).sqrt().min(1.0)
            } else {
                rms
            };

            let is_calibrating = frame_count < 30;
            let is_quiet = calibration_rms < noise_floor_ema * 3.0 || noise_floor_ema < 0.0001;

            if is_calibrating || is_quiet {
                if noise_floor_ema < 0.0001 {
                    noise_floor_ema = calibration_rms;
                } else {
                    noise_floor_ema =
                        ema_alpha * calibration_rms + (1.0 - ema_alpha) * noise_floor_ema;
                }
            }

            // Set threshold at 15x the noise floor for clear separation.
            if frame_count > 15 && frame_count.is_multiple_of(10) {
                let threshold = (noise_floor_ema * 15.0).clamp(0.03, 0.5);
                if let Ok(mut state) = inner.lock() {
                    if (state.audio_settings.vad_threshold - threshold).abs() > 0.002 {
                        state.audio_settings.vad_threshold = threshold;
                        let _ = app.emit("vad-threshold-updated", threshold);
                    }
                }
            }
        }
    }
}

// -----------------------------------------------------------------------
//  Latency test
// -----------------------------------------------------------------------

/// Background task that sends TCP pings at ~2 Hz so the event handler can
/// compute RTT and emit `"ping-latency"` events for the latency graph.
pub(super) async fn latency_ping_loop(client_handle: ClientHandle) {
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
