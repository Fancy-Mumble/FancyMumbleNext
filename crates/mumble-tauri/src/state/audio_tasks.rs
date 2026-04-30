//! Background async tasks for audio: outbound encoding/sending loop,
//! mic test loop with auto-calibration, and latency ping loop.
//!
//! Extracted from `audio.rs` to keep file sizes manageable.

use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, warn};

use mumble_protocol::audio::capture::AudioCapture;
use mumble_protocol::audio::filter::automatic_gain::AutomaticGainControl;
use mumble_protocol::audio::pipeline::{OutboundPipeline, OutboundTick};
use mumble_protocol::client::ClientHandle;
use mumble_protocol::command;
use mumble_protocol::message::UdpMessage;
use mumble_protocol::proto::mumble_udp;

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

/// When dropped, emits a `user-talking` = false event so the frontend
/// clears the self-talking indicator if the task is cancelled.
struct TalkingGuard {
    app: Option<tauri::AppHandle>,
    session: Option<u32>,
    is_talking: bool,
}

impl Drop for TalkingGuard {
    fn drop(&mut self) {
        if self.is_talking {
            if let (Some(app), Some(session)) = (&self.app, self.session) {
                use tauri::Emitter;
                let _ = app.emit("user-talking", (session, false));
            }
        }
    }
}

/// Background task that reads from the microphone, encodes, and queues
/// Opus packets for network transmission.
///
/// Encoding and network I/O are decoupled via a bounded channel so
/// that slow network sends never stall the audio processing loop.
///
/// When `app` and `own_session` are provided, the loop emits
/// `user-talking` events so the frontend can show a self-talking
/// indicator.
pub(super) async fn outbound_audio_loop(
    mut pipeline: OutboundPipeline,
    handle: ClientHandle,
    app: Option<tauri::AppHandle>,
    own_session: Option<u32>,
) {
    debug!("outbound_audio_loop: task started");

    // Start the capture device inside the encoding task so the cpal
    // stream only begins producing samples when we are ready to consume.
    if let Err(e) = pipeline.start() {
        warn!("outbound_audio_loop: capture start failed: {e}");
        return;
    }

    // Bounded channel: 50 packets ~ 1 second of audio at 20ms/frame.
    let (tx, rx) = tokio::sync::mpsc::channel::<AudioPacketOut>(50);

    let _outbound_send_task = tokio::spawn(outbound_send_task(rx, handle));

    // Brief yield so the cpal callback can deliver an initial batch of
    // samples, then drain any that accumulated during startup.
    tokio::task::yield_now().await;
    while pipeline.tick().is_ok_and(|t| !matches!(t, OutboundTick::NoData)) {}

    debug!("outbound_audio_loop: entering encoding loop");

    // Poll at 5 ms instead of 20 ms. Each frame is still 960 samples
    // (20 ms @ 48 kHz), but the shorter interval reduces the chance of
    // missing the moment when enough samples become available.
    let mut interval = tokio::time::interval(Duration::from_millis(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut stats = OutboundStats::default();
    let mut last_tick = tokio::time::Instant::now();
    let mut guard = TalkingGuard {
        app: app.clone(),
        session: own_session,
        is_talking: false,
    };

    loop {
        let _ = interval.tick().await;

        // Detect when the Tokio runtime was stalled (e.g. Android
        // throttled the process).  Skip this tick so the capture
        // `read_frame()` overflow logic can discard stale audio
        // instead of encoding it all in a burst.
        let now = tokio::time::Instant::now();
        let elapsed = now.duration_since(last_tick);
        last_tick = now;
        if elapsed > Duration::from_millis(200) {
            warn!(
                "outbound_audio: tick delayed by {:.0} ms, skipping to discard stale audio",
                elapsed.as_millis()
            );
            while pipeline.tick().is_ok_and(|t| !matches!(t, OutboundTick::NoData)) {}
            continue;
        }

        // Process a bounded number of frames per tick.
        for _ in 0..5 {
            if !process_outbound_tick(
                &mut pipeline,
                &tx,
                &app,
                own_session,
                &mut guard,
                &mut stats,
            ) {
                break;
            }
        }
    }
}

/// Counters for periodic outbound audio diagnostics.
#[derive(Default)]
struct OutboundStats {
    packets: u64,
    silence: u64,
    total: u64,
}

/// Process a single outbound pipeline tick.
///
/// Returns `true` when more frames may be available in the same tick
/// batch, `false` when the caller should stop the inner loop.
fn process_outbound_tick(
    pipeline: &mut OutboundPipeline,
    tx: &tokio::sync::mpsc::Sender<AudioPacketOut>,
    app: &Option<tauri::AppHandle>,
    own_session: Option<u32>,
    guard: &mut TalkingGuard,
    stats: &mut OutboundStats,
) -> bool {
    use tauri::Emitter;

    match pipeline.tick() {
        Ok(OutboundTick::Audio(packet)) => {
            stats.packets += 1;
            stats.total += 1;
            if !guard.is_talking {
                guard.is_talking = true;
                if let (Some(app), Some(session)) = (app, own_session) {
                    let _ = app.emit("user-talking", (session, true));
                }
            }
            if stats.packets == 1 || stats.packets.is_multiple_of(500) {
                debug!(
                    "outbound_audio: sending packet #{} (opus {} bytes, seq {})",
                    stats.packets,
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
            true
        }
        Ok(OutboundTick::Terminator(packet)) => {
            stats.total += 1;
            if guard.is_talking {
                guard.is_talking = false;
                if let (Some(app), Some(session)) = (app, own_session) {
                    let _ = app.emit("user-talking", (session, false));
                }
            }
            debug!(
                "outbound_audio: sending terminator (opus {} bytes)",
                packet.data.len()
            );
            let _ = tx.try_send(AudioPacketOut {
                data: packet.data,
                sequence: packet.sequence,
                is_terminator: true,
            });
            true
        }
        Ok(OutboundTick::Silence) => {
            stats.silence += 1;
            stats.total += 1;
            if stats.total.is_multiple_of(500) {
                debug!(
                    "outbound_audio stats: total={}, sent={}, silenced={}, silence_rate={:.1}%",
                    stats.total,
                    stats.packets,
                    stats.silence,
                    if stats.total > 0 {
                        stats.silence as f64 / stats.total as f64 * 100.0
                    } else {
                        0.0
                    },
                );
            }
            true
        }
        Ok(OutboundTick::NoData) => false,
        Err(e) => {
            warn!("outbound audio error: {e}");
            false
        }
    }
}

/// Drains encoded audio packets from the channel and sends them to
/// the server via the high-priority audio path.
async fn outbound_send_task(
    mut rx: tokio::sync::mpsc::Receiver<AudioPacketOut>,
    handle: ClientHandle,
) {
    let mut sent: u64 = 0;
    let mut dropped: u64 = 0;
    while let Some(pkt) = rx.recv().await {
        let audio = mumble_udp::Audio {
            header: Some(mumble_udp::audio::Header::Target(0)),
            sender_session: 0,
            frame_number: pkt.sequence,
            opus_data: pkt.data,
            positional_data: Vec::new(),
            volume_adjustment: 0.0,
            is_terminator: pkt.is_terminator,
        };
        match handle.send_audio(UdpMessage::Audio(audio)) {
            Ok(()) => {
                sent += 1;
                if sent == 1 {
                    debug!("outbound_send_task: first packet queued to event loop");
                }
            }
            Err(e) => {
                dropped += 1;
                if dropped == 1 || dropped.is_multiple_of(100) {
                    warn!(
                        "outbound_audio: send failed (dropped={dropped}, sent={sent}): {e}",
                    );
                }
            }
        }
    }
    debug!("outbound_send_task: channel closed, sent={sent}, dropped={dropped}");
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
                let should_emit = update_vad_threshold_if_changed(&inner, threshold);
                if should_emit {
                    let _ = app.emit("vad-threshold-updated", threshold);
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

fn update_vad_threshold_if_changed(inner: &std::sync::Mutex<SharedState>, threshold: f32) -> bool {
    let Ok(mut state) = inner.lock() else { return false };
    if (state.audio.settings.vad_threshold - threshold).abs() > 0.002 {
        state.audio.settings.vad_threshold = threshold;
        true
    } else {
        false
    }
}
