//! Rodio-based audio capture and mixing playback implementations.
//!
//! Alternative desktop audio backend using rodio (which wraps cpal
//! internally but provides a higher-level, push-based API).  This
//! avoids the low-level callback complexity of raw cpal and lets
//! rodio handle device threading, sample-rate conversion, and mixing.

use std::collections::VecDeque;
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread;

const MONO_CHANNELS: NonZero<u16> = NonZero::new(1).unwrap();
const SAMPLE_RATE_48K: NonZero<u32> = NonZero::new(48_000).unwrap();

use mumble_protocol::audio::capture::AudioCapture;
use mumble_protocol::audio::mixer::{SpeakerBuffers, SpeakerVolumes};
use mumble_protocol::audio::sample::{AudioFormat, AudioFrame};
use mumble_protocol::error::{Error, Result};
use rodio::microphone::MicrophoneBuilder;
use rodio::source::Source;
use tracing::debug;

// -- Capture (Microphone) -------------------------------------------

/// Captures microphone input via rodio's [`Microphone`] source using a
/// background thread so that [`read_frame`](AudioCapture::read_frame)
/// is non-blocking.
///
/// Rodio's `Microphone::next()` blocks until hardware delivers a
/// sample, which is incompatible with the outbound pipeline's drain
/// loops (they expect `NotEnoughSamples` when no data is ready).
/// A dedicated thread reads from the Microphone and sends mono `f32`
/// samples through a bounded channel.  `read_frame()` drains the
/// channel with `try_recv()` and returns `NotEnoughSamples` when fewer
/// than `frame_size` samples are available.
pub struct RodioCapture {
    format: AudioFormat,
    frame_size: usize,
    sequence: u64,
    sample_rx: Option<mpsc::Receiver<f32>>,
    _capture_thread: Option<thread::JoinHandle<()>>,
    volume: Arc<AtomicU32>,
    pending: Vec<f32>,
}

impl RodioCapture {
    pub fn new(
        _device_name: Option<&str>,
        frame_size: usize,
        volume: Arc<AtomicU32>,
    ) -> Result<Self> {
        Ok(Self {
            format: AudioFormat::MONO_48KHZ_F32,
            frame_size,
            sequence: 0,
            sample_rx: None,
            _capture_thread: None,
            volume,
            pending: Vec::with_capacity(frame_size * 2),
        })
    }
}

impl AudioCapture for RodioCapture {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn read_frame(&mut self) -> Result<AudioFrame> {
        let rx = self
            .sample_rx
            .as_ref()
            .ok_or_else(|| Error::InvalidState("Microphone not started".into()))?;

        loop {
            match rx.try_recv() {
                Ok(sample) => self.pending.push(sample),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(Error::InvalidState("Microphone stream ended".into()));
                }
            }
        }

        if self.pending.len() < self.frame_size {
            return Err(Error::NotEnoughSamples);
        }

        let vol = f32::from_bits(self.volume.load(Ordering::Relaxed));
        let samples: Vec<f32> = self
            .pending
            .drain(..self.frame_size)
            .map(|s| s * vol)
            .collect();

        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_ne_bytes()).collect();
        self.sequence += 1;
        Ok(AudioFrame {
            data,
            format: self.format,
            sequence: self.sequence,
            is_silent: false,
        })
    }

    fn start(&mut self) -> Result<()> {
        let builder = MicrophoneBuilder::new()
            .default_device()
            .map_err(|e| Error::InvalidState(format!("No input device: {e}")))?
            .default_config()
            .map_err(|e| Error::InvalidState(format!("Input config: {e}")))?
            .prefer_channel_counts([MONO_CHANNELS])
            .prefer_sample_rates([SAMPLE_RATE_48K]);

        let mic = builder
            .open_stream()
            .map_err(|e| Error::InvalidState(format!("Open microphone: {e}")))?;

        let config = mic.config();
        debug!(
            "rodio microphone opened: rate={}, channels={}",
            config.sample_rate.get(),
            config.channel_count.get(),
        );

        let mic_channels = config.channel_count.get() as usize;

        let (tx, rx) = mpsc::sync_channel::<f32>(48_000);

        let handle = thread::Builder::new()
            .name("rodio-mic-reader".into())
            .spawn(move || capture_thread(mic, mic_channels, tx))
            .map_err(|e| Error::InvalidState(format!("Spawn mic thread: {e}")))?;

        self.sample_rx = Some(rx);
        self._capture_thread = Some(handle);
        self.pending.clear();
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.sample_rx = None;
        self._capture_thread = None;
        self.pending.clear();
        Ok(())
    }
}

fn capture_thread(
    mut mic: rodio::microphone::Microphone,
    channels: usize,
    tx: SyncSender<f32>,
) {
    loop {
        let mut sum = 0.0f32;
        for _ in 0..channels {
            match mic.next() {
                Some(s) => sum += s,
                None => return,
            }
        }
        let mono = sum / channels as f32;
        if tx.send(mono).is_err() {
            return;
        }
    }
}

// -- Custom Source for Mumble audio mixing --------------------------

/// Number of mono samples to mix per refill (20 ms at 48 kHz).
const MIX_CHUNK_SIZE: usize = 960;
/// Minimum buffered samples before playback begins (~100 ms).
const PRE_BUFFER_SAMPLES: usize = 4800;
/// Refill back-off (in mono samples) when a refill returns no data.
/// 5 ms keeps the speaker buffer mutex contention bounded (max
/// ~200 lock attempts/s per source) while letting a transient jitter
/// recover within a few ms instead of forcing a full 20 ms of decay.
const UNDERRUN_BACKOFF_SAMPLES: usize = 240;
/// Consecutive empty refills before re-priming the buffer.  Each
/// empty refill represents one [`UNDERRUN_BACKOFF_SAMPLES`] period
/// (5 ms), so 300 corresponds to ~1.5 s of sustained silence.
const REPRIME_AFTER: u32 = 300;

/// A rodio [`Source`] that reads from per-speaker ring buffers and
/// yields mixed mono `f32` samples at 48 kHz.
///
/// rodio's background output thread calls `Iterator::next()` to pull
/// samples. Mixing is done in chunks of [`MIX_CHUNK_SIZE`] to avoid
/// locking the speaker buffers on every single sample.
struct MumbleMixerSource {
    buffers: SpeakerBuffers,
    speaker_volumes: SpeakerVolumes,
    volume: Arc<AtomicU32>,
    mixed_chunk: Vec<f32>,
    chunk_pos: usize,
    chunk_valid: usize,
    primed: bool,
    consecutive_empty: u32,
    running: Arc<AtomicBool>,
    last_sample: f32,
    in_underrun: bool,
    ramp_pos: usize,
    underrun_samples: usize,
    /// Amplitude captured at the moment underrun begins.  Used as the
    /// starting point of the cosine fade-out toward silence.  Without
    /// a fade the abrupt step from `last_sample` to 0 (or holding
    /// `last_sample` at a non-zero DC value before decay) creates a
    /// broadband click - especially audible at the end of a short
    /// utterance like a single word.
    fade_anchor: f32,
    /// Remaining samples to skip before the next `refill_chunk()` call.
    /// Prevents per-sample refill attempts during underrun by throttling
    /// them to one every [`UNDERRUN_BACKOFF_SAMPLES`] (5 ms).  Short
    /// enough that transient jitter recovers within a few ms instead
    /// of forcing a full chunk of decay/silence.
    underrun_cooldown: usize,
    diag: MixerDiag,
}

/// Periodic diagnostics for the rodio mixer source.
struct MixerDiag {
    samples_pulled: u64,
    refills: u64,
    underrun_refills: u64,
    partial_refills: u64,
    ramps_applied: u64,
    peak: f32,
    max_buf_depth: usize,
    lock_failures: u64,
    reprime_count: u64,
}

impl MumbleMixerSource {
    fn new(
        buffers: SpeakerBuffers,
        speaker_volumes: SpeakerVolumes,
        volume: Arc<AtomicU32>,
        running: Arc<AtomicBool>,
    ) -> Self {
        Self {
            buffers,
            speaker_volumes,
            volume,
            mixed_chunk: vec![0.0; MIX_CHUNK_SIZE],
            chunk_pos: 0,
            chunk_valid: 0,
            primed: false,
            consecutive_empty: 0,
            running,
            last_sample: 0.0,
            in_underrun: false,
            ramp_pos: 0,
            underrun_samples: 0,
            fade_anchor: 0.0,
            underrun_cooldown: 0,
            diag: MixerDiag {
                samples_pulled: 0,
                refills: 0,
                underrun_refills: 0,
                partial_refills: 0,
                ramps_applied: 0,
                peak: 0.0,
                max_buf_depth: 0,
                lock_failures: 0,
                reprime_count: 0,
            },
        }
    }

    fn refill_chunk(&mut self) {
        let Ok(mut bufs) = self.buffers.lock() else {
            self.diag.lock_failures += 1;
            self.chunk_pos = 0;
            self.chunk_valid = 0;
            self.underrun_cooldown = UNDERRUN_BACKOFF_SAMPLES;
            return;
        };

        if !self.primed {
            let max_available = bufs.values().map(VecDeque::len).max().unwrap_or(0);
            if max_available < PRE_BUFFER_SAMPLES {
                self.chunk_pos = 0;
                self.chunk_valid = 0;
                self.underrun_cooldown = UNDERRUN_BACKOFF_SAMPLES;
                return;
            }
            self.primed = true;
        }

        let sv = self
            .speaker_volumes
            .try_lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        let (drained, valid_count, buf_depth) =
            super::desktop::batch_drain_speakers(&mut bufs, &sv, &mut self.mixed_chunk, MIX_CHUNK_SIZE);

        self.diag.refills += 1;
        self.diag.max_buf_depth = self.diag.max_buf_depth.max(buf_depth);

        if !drained || valid_count == 0 {
            self.consecutive_empty += 1;
            self.diag.underrun_refills += 1;
            if self.consecutive_empty >= REPRIME_AFTER {
                self.primed = false;
                self.diag.reprime_count += 1;
            }
            self.chunk_pos = 0;
            self.chunk_valid = 0;
            self.underrun_cooldown = UNDERRUN_BACKOFF_SAMPLES;
        } else {
            if valid_count < MIX_CHUNK_SIZE {
                self.diag.partial_refills += 1;
            }
            self.consecutive_empty = 0;
            self.chunk_pos = 0;
            self.chunk_valid = valid_count;
        }
    }
}

impl Iterator for MumbleMixerSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if !self.running.load(Ordering::Relaxed) {
            return None;
        }

        if self.underrun_cooldown > 0 {
            self.underrun_cooldown -= 1;
        } else if self.chunk_pos >= self.chunk_valid {
            self.refill_chunk();
        }

        let vol = f32::from_bits(self.volume.load(Ordering::Relaxed));

        self.diag.samples_pulled += 1;
        // Log diagnostics every ~1 second (48000 samples at 48 kHz).
        if self.diag.samples_pulled.is_multiple_of(48_000) {
            debug!(
                "rodio mixer diag: pulled={}, refills={}, underrun={}, partial={}, \
                 ramps={}, peak={:.4}, max_buf={}, lock_fail={}, reprimes={}, \
                 consec_empty={}, in_underrun={}",
                self.diag.samples_pulled,
                self.diag.refills,
                self.diag.underrun_refills,
                self.diag.partial_refills,
                self.diag.ramps_applied,
                self.diag.peak,
                self.diag.max_buf_depth,
                self.diag.lock_failures,
                self.diag.reprime_count,
                self.consecutive_empty,
                self.in_underrun,
            );
        }

        if self.chunk_pos < self.chunk_valid {
            let raw = self.mixed_chunk[self.chunk_pos];
            self.chunk_pos += 1;

            let sample = if self.in_underrun {
                self.diag.ramps_applied += 1;
                // Scale the fade-in length with how deep the silence was:
                // a brief jitter recovers in ~1 ms, but coming out of
                // a multi-second silence (e.g. someone starts talking
                // for the first time) requires a much longer fade or
                // the abrupt onset is heard as a click/pop at the
                // ramp's fundamental frequency.
                const MIN_RAMP: usize = 48;   // ~1 ms - jitter recovery
                const MAX_RAMP: usize = 480;  // ~10 ms - speech onset
                let ramp_len = self.underrun_samples.clamp(MIN_RAMP, MAX_RAMP);
                self.ramp_pos += 1;
                if self.ramp_pos >= ramp_len {
                    self.in_underrun = false;
                    self.ramp_pos = 0;
                    self.underrun_samples = 0;
                    raw
                } else {
                    // Equal-power cosine fade: smoother perceptually
                    // than linear, no derivative discontinuity at the
                    // endpoints, and avoids the audible "ramp tone"
                    // that a short linear fade can produce.
                    let t = self.ramp_pos as f32 / ramp_len as f32;
                    let w = 0.5 - 0.5 * (std::f32::consts::PI * t).cos();
                    self.last_sample * (1.0 - w) + raw * w
                }
            } else {
                raw
            };

            self.diag.peak = self.diag.peak.max(sample.abs());
            self.last_sample = sample;
            Some(super::soft_clip(sample * vol))
        } else {
            // Underrun strategy: cosine fade-out from the amplitude
            // we held at the moment underrun began (`fade_anchor`)
            // toward zero over `FADE_OUT_LEN` samples, then silence.
            //
            // Why a cosine fade instead of DC-hold-then-decay:
            // holding a non-zero DC value for a few ms (e.g. when a
            // speaker stops mid-word) is itself a step in the
            // waveform, which is broadband and audible as a click.
            // The cosine fade has zero derivative at both endpoints
            // and decays smoothly from `fade_anchor` to 0, so the
            // end of an utterance is inaudible and brief mid-word
            // jitter only causes a small, smooth amplitude dip
            // (~15 % at 5 ms, fully recovers when audio resumes).
            const FADE_OUT_LEN: usize = 960; // 20 ms at 48 kHz

            // Capture the anchor on the first underrun sample only;
            // this preserves the smooth fade across the entire gap
            // even when individual `next()` calls are interleaved
            // with cooldown decrements.
            if !self.in_underrun {
                self.fade_anchor = self.last_sample;
            }
            self.in_underrun = true;
            self.ramp_pos = 0;
            self.underrun_samples += 1;

            if self.underrun_samples >= FADE_OUT_LEN {
                self.last_sample = 0.0;
            } else {
                let t = self.underrun_samples as f32 / FADE_OUT_LEN as f32;
                let w = 0.5 + 0.5 * (std::f32::consts::PI * t).cos();
                self.last_sample = self.fade_anchor * w;
            }
            Some(super::soft_clip(self.last_sample * vol))
        }
    }
}

impl Source for MumbleMixerSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> NonZero<u16> {
        MONO_CHANNELS
    }

    fn sample_rate(&self) -> NonZero<u32> {
        SAMPLE_RATE_48K
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

// -- Mixing Playback ------------------------------------------------

/// Rodio-based mixing playback device.
///
/// Opens the default output via rodio's `Mixer` and feeds a
/// [`MumbleMixerSource`] that reads from the shared per-speaker
/// buffers. Rodio handles the output thread, sample-rate conversion
/// (if the device is not 48 kHz), and buffer management.
pub struct RodioMixingPlayback {
    /// Keeps the device sink alive; dropping it stops playback.
    _device_sink: Option<rodio::stream::MixerDeviceSink>,
    running: Arc<AtomicBool>,
    buffers: SpeakerBuffers,
    speaker_volumes: SpeakerVolumes,
    volume: Arc<AtomicU32>,
}

impl RodioMixingPlayback {
    pub fn new(
        _device_name: Option<&str>,
        volume: Arc<AtomicU32>,
        buffers: SpeakerBuffers,
        speaker_volumes: SpeakerVolumes,
    ) -> Result<Self> {
        Ok(Self {
            _device_sink: None,
            running: Arc::new(AtomicBool::new(false)),
            buffers,
            speaker_volumes,
            volume,
        })
    }
}

impl super::MixingPlayback for RodioMixingPlayback {
    fn start(&mut self) -> Result<()> {
        // Open the default output device and get a mixer handle.
        let device_sink = rodio::stream::DeviceSinkBuilder::from_default_device()
            .map_err(|e| Error::InvalidState(format!("Open output device: {e}")))?
            .open_stream()
            .map_err(|e| Error::InvalidState(format!("Open output stream: {e}")))?;
        let mixer = device_sink.mixer().clone();

        let cfg = device_sink.config();
        debug!(
            "rodio playback opened: device_rate={} Hz, channels={}",
            cfg.sample_rate().get(),
            cfg.channel_count().get(),
        );

        self.running.store(true, Ordering::Relaxed);

        let source = MumbleMixerSource::new(
            self.buffers.clone(),
            self.speaker_volumes.clone(),
            self.volume.clone(),
            self.running.clone(),
        );
        mixer.add(source);

        self._device_sink = Some(device_sink);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);
        self._device_sink = None;
        if let Ok(mut bufs) = self.buffers.lock() {
            bufs.values_mut().for_each(VecDeque::clear);
        }
        Ok(())
    }
}

// -- Factory --------------------------------------------------------

/// Desktop audio device factory backed by rodio.
pub struct RodioAudioFactory;

impl super::AudioDeviceFactory for RodioAudioFactory {
    fn create_capture(
        device_name: Option<&str>,
        frame_size: usize,
        volume: Arc<AtomicU32>,
    ) -> std::result::Result<Box<dyn AudioCapture>, String> {
        RodioCapture::new(device_name, frame_size, volume)
            .map(|c| Box::new(c) as _)
            .map_err(|e| format!("Rodio capture init: {e}"))
    }

    fn create_mixing_playback(
        device_name: Option<&str>,
        volume: Arc<AtomicU32>,
        buffers: SpeakerBuffers,
        speaker_volumes: SpeakerVolumes,
    ) -> std::result::Result<Box<dyn super::MixingPlayback>, String> {
        RodioMixingPlayback::new(device_name, volume, buffers, speaker_volumes)
            .map(|c| Box::new(c) as _)
            .map_err(|e| format!("Rodio mixing playback init: {e}"))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn make_source(buffers: SpeakerBuffers, speaker_volumes: SpeakerVolumes) -> MumbleMixerSource {
        let volume = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
        let running = Arc::new(AtomicBool::new(true));
        MumbleMixerSource::new(buffers, speaker_volumes, volume, running)
    }

    #[test]
    fn underrun_decays_smoothly_instead_of_hard_silence() {
        let buffers: SpeakerBuffers = Arc::new(Mutex::new(HashMap::new()));
        let svols: SpeakerVolumes = Arc::new(Mutex::new(HashMap::new()));

        let total_samples = PRE_BUFFER_SAMPLES + MIX_CHUNK_SIZE;
        {
            let mut bufs = buffers.lock().unwrap();
            let buf = bufs.entry(1).or_default();
            for _ in 0..total_samples {
                buf.push_back(0.5);
            }
        }

        let mut src = make_source(buffers, svols);

        // Drain all real audio samples.
        for _ in 0..total_samples {
            let s = src.next().unwrap();
            assert!((s - 0.5).abs() < 0.01, "expected ~0.5, got {s}");
        }

        // First underrun sample: cosine fade-out at t=1/960 starts
        // essentially at the anchor amplitude (~0.5), NOT a hard 0.
        let first_underrun = src.next().unwrap();
        assert!(
            first_underrun.abs() > 0.4,
            "first underrun sample should still be near anchor 0.5, got {first_underrun}"
        );

        // After the full fade-out length (960 samples = 20 ms),
        // output should be silence.
        for _ in 0..960 {
            let _ = src.next();
        }
        let after_fade = src.next().unwrap();
        assert!(
            after_fade.abs() < 1e-3,
            "after fade-out should be silence, got {after_fade}"
        );
    }

    #[test]
    fn resume_after_underrun_ramps_smoothly() {
        let buffers: SpeakerBuffers = Arc::new(Mutex::new(HashMap::new()));
        let svols: SpeakerVolumes = Arc::new(Mutex::new(HashMap::new()));

        let total_samples = PRE_BUFFER_SAMPLES + MIX_CHUNK_SIZE;
        {
            let mut bufs = buffers.lock().unwrap();
            let buf = bufs.entry(1).or_default();
            for _ in 0..total_samples {
                buf.push_back(0.5);
            }
        }

        let mut src = make_source(buffers.clone(), svols);

        // Drain all real audio.
        for _ in 0..total_samples {
            let _ = src.next();
        }

        // Enter underrun for 20 samples (short underrun -> ramp_len = 20).
        for _ in 0..20 {
            let _ = src.next();
        }
        assert!(src.in_underrun, "should be in underrun state");

        // Refill speaker buffer with new audio at a different level.
        {
            let mut bufs = buffers.lock().unwrap();
            let buf = bufs.entry(1).or_default();
            for _ in 0..MIX_CHUNK_SIZE {
                buf.push_back(-0.3);
            }
        }

        // Drain remaining cooldown - the source continues decay output
        // until the next refill attempt at the chunk boundary.  Each
        // of these samples also bumps `underrun_samples`, deepening
        // the perceived underrun for the upcoming ramp.
        let remaining = src.underrun_cooldown;
        for _ in 0..remaining {
            let _ = src.next();
        }
        let expected_ramp = src.underrun_samples.clamp(48, 480);

        // First samples after resume should ramp from decayed last_sample
        // toward -0.3, NOT jump directly to -0.3.
        let resume_sample = src.next().unwrap();
        // The cosine fade at t=1/ramp_len starts essentially at last_sample,
        // so the first ramp output should be far closer to ~0.49 than to -0.3.
        assert!(
            resume_sample > -0.2,
            "resume should ramp, not jump to -0.3; got {resume_sample}"
        );

        // After the full ramp completes, samples should be ~-0.3.
        // Drain enough samples to be safely past the ramp end.
        for _ in 0..expected_ramp {
            let _ = src.next();
        }
        let settled = src.next().unwrap();
        assert!(
            (settled - (-0.3)).abs() < 0.05,
            "after ramp ({expected_ramp} samples) should settle to -0.3, got {settled}"
        );
    }

    #[test]
    fn brief_underrun_does_not_trigger_reprime() {
        let buffers: SpeakerBuffers = Arc::new(Mutex::new(HashMap::new()));
        let svols: SpeakerVolumes = Arc::new(Mutex::new(HashMap::new()));

        let total_samples = PRE_BUFFER_SAMPLES + MIX_CHUNK_SIZE;
        {
            let mut bufs = buffers.lock().unwrap();
            let buf = bufs.entry(1).or_default();
            for _ in 0..total_samples {
                buf.push_back(0.5);
            }
        }

        let mut src = make_source(buffers.clone(), svols);

        for _ in 0..total_samples {
            let _ = src.next();
        }
        assert!(src.primed, "should be primed after initial playback");

        // Drain one full chunk's worth of underrun (MIX_CHUNK_SIZE samples).
        // With the 5 ms back-off, this counts as
        // MIX_CHUNK_SIZE / UNDERRUN_BACKOFF_SAMPLES empty refills.
        for _ in 0..MIX_CHUNK_SIZE {
            let _ = src.next();
        }

        let expected_empty = (MIX_CHUNK_SIZE / UNDERRUN_BACKOFF_SAMPLES) as u32;
        assert!(src.primed, "should still be primed after a single-chunk underrun");
        assert_eq!(
            src.consecutive_empty, expected_empty,
            "one chunk of underrun should count as {expected_empty} empty refills, not {}", src.consecutive_empty
        );
        assert_eq!(src.diag.reprime_count, 0, "no repriming should have occurred");
        assert!(
            expected_empty < REPRIME_AFTER,
            "REPRIME_AFTER ({REPRIME_AFTER}) must be larger than a single-chunk underrun ({expected_empty})"
        );
    }

    #[test]
    fn speech_onset_after_long_silence_uses_long_fade_in() {
        // Regression: when audio starts after the source has been
        // silent for a long time (typical "user starts talking"), the
        // fade-in must be long enough that the onset is not perceived
        // as a click/pop.  A 1 ms ramp from 0.0 to ~0.5 produces an
        // audible transient at ~500 Hz; we want at least ~10 ms.
        let buffers: SpeakerBuffers = Arc::new(Mutex::new(HashMap::new()));
        let svols: SpeakerVolumes = Arc::new(Mutex::new(HashMap::new()));

        let mut src = make_source(buffers.clone(), svols);

        // Pull enough samples to be deeply in underrun (simulate
        // ~1 second of silence before any audio arrives).
        for _ in 0..48_000 {
            let _ = src.next();
        }
        assert!(src.in_underrun, "should be in underrun after long silence");
        assert!(
            src.underrun_samples > 1_000,
            "underrun_samples should accumulate during long silence, got {}",
            src.underrun_samples
        );

        // Now simulate a speaker beginning to talk: fill the buffer.
        {
            let mut bufs = buffers.lock().unwrap();
            let buf = bufs.entry(1).or_default();
            for _ in 0..(PRE_BUFFER_SAMPLES + MIX_CHUNK_SIZE) {
                buf.push_back(0.5);
            }
        }

        // Drain pending cooldown so the next call refills the chunk.
        let cooldown = src.underrun_cooldown;
        for _ in 0..cooldown {
            let _ = src.next();
        }

        // Confirm the ramp will use the long-silence path (>=480 samples).
        // After the cooldown drain underrun_samples is huge, so the
        // ramp_len clamps at MAX_RAMP=480.
        assert!(
            src.underrun_samples >= 480,
            "underrun must be deep enough to trigger MAX_RAMP, got {}",
            src.underrun_samples
        );

        // The first 48 samples (the OLD ramp_len) must NOT yet have
        // approached the target amplitude - that was the bug.  With a
        // 480-sample cosine fade, the first 48 samples (10% of the
        // ramp) keep the output well below half target.
        let mut peak_in_first_ms = 0.0_f32;
        for _ in 0..48 {
            let s = src.next().unwrap();
            peak_in_first_ms = peak_in_first_ms.max(s.abs());
        }
        assert!(
            peak_in_first_ms < 0.20,
            "first 1 ms of fade-in should stay well below target 0.5, got peak {peak_in_first_ms}"
        );

        // After enough samples to clearly clear the ramp, output must
        // have reached the target amplitude ~0.5.
        for _ in 0..MAX_RAMP_GUARD {
            let _ = src.next();
        }
        let settled = src.next().unwrap();
        assert!(
            (settled - 0.5).abs() < 0.02,
            "after fade-in completes, should reach target 0.5, got {settled}"
        );
        assert!(!src.in_underrun, "ramp should be complete by now");
    }

    /// A safety margin larger than `MAX_RAMP` (480 samples) used by
    /// fade-in tests to drain past the cosine fade.
    const MAX_RAMP_GUARD: usize = 600;

    #[test]
    fn end_of_utterance_fades_smoothly_no_step() {
        // Regression: when a speaker stops mid-word (single-word
        // utterance), the buffer suddenly empties at a non-zero
        // amplitude.  Holding that amplitude flat for any duration
        // creates a DC step in the waveform, audible as a click.
        // The cosine fade-out must produce a strictly monotonic,
        // step-free decay from the anchor amplitude to zero.
        let buffers: SpeakerBuffers = Arc::new(Mutex::new(HashMap::new()));
        let svols: SpeakerVolumes = Arc::new(Mutex::new(HashMap::new()));

        // Fill with a constant non-zero amplitude (e.g. peak of a
        // word's vowel formant).
        let total_samples = PRE_BUFFER_SAMPLES + MIX_CHUNK_SIZE;
        const ANCHOR: f32 = 0.7;
        {
            let mut bufs = buffers.lock().unwrap();
            let buf = bufs.entry(1).or_default();
            for _ in 0..total_samples {
                buf.push_back(ANCHOR);
            }
        }

        let mut src = make_source(buffers, svols);
        for _ in 0..total_samples {
            let _ = src.next();
        }

        // Collect the full fade-out window plus a margin.
        let mut samples = Vec::with_capacity(1100);
        for _ in 0..1100 {
            samples.push(src.next().unwrap());
        }

        // Step 1: the very first underrun sample must NOT be zero
        // (which would be a hard cliff) and must NOT exceed the
        // anchor (which would be amplification).
        assert!(
            samples[0] > 0.5 && samples[0] <= ANCHOR + 1e-3,
            "first fade-out sample should start near anchor, got {}", samples[0]
        );

        // Step 2: monotonically decreasing amplitude (no plateau,
        // no oscillation) for the full fade window.
        for i in 1..960 {
            assert!(
                samples[i] <= samples[i - 1] + 1e-4,
                "fade-out must be monotonic; samples[{}]={} > samples[{}]={}",
                i, samples[i], i - 1, samples[i - 1]
            );
        }

        // Step 3: after the fade-out length, output must be at zero
        // (true silence, not a residual DC offset).
        for &s in &samples[1000..] {
            assert!(
                s.abs() < 1e-4,
                "after fade-out must be exactly silence, got {s}"
            );
        }

        // Step 4: there must be no step larger than ~1.5 % of the
        // anchor between consecutive samples (smooth derivative).
        const MAX_STEP: f32 = 0.012;
        for i in 1..960 {
            let delta = (samples[i] - samples[i - 1]).abs();
            assert!(
                delta < MAX_STEP,
                "step between samples[{}] and samples[{}] = {} exceeds smoothness budget {}",
                i - 1, i, delta, MAX_STEP
            );
        }
    }

    #[test]
    fn brief_underrun_holds_amplitude_then_decays() {
        // Regression: a 1-2 packet network jitter spike in the middle
        // of a word must NOT cause a hard step in amplitude (which
        // sounds like a click/warble).  The cosine fade-out keeps
        // the first ~5 ms of underrun at >= 85 % of the anchor
        // amplitude, so brief jitter is barely audible.
        let buffers: SpeakerBuffers = Arc::new(Mutex::new(HashMap::new()));
        let svols: SpeakerVolumes = Arc::new(Mutex::new(HashMap::new()));

        let total_samples = PRE_BUFFER_SAMPLES + MIX_CHUNK_SIZE;
        {
            let mut bufs = buffers.lock().unwrap();
            let buf = bufs.entry(1).or_default();
            for _ in 0..total_samples {
                buf.push_back(0.5);
            }
        }

        let mut src = make_source(buffers.clone(), svols);
        for _ in 0..total_samples {
            let _ = src.next();
        }
        // Now in steady-state, last_sample = 0.5.

        // 5 ms (240 samples) of underrun: cosine fade at t=240/960=0.25
        // gives w = 0.5 + 0.5*cos(pi*0.25) ~= 0.854, so the sample at
        // 240 should be ~0.427.  Throughout the first 5 ms the
        // amplitude must stay above ~80 % of the anchor.
        for i in 0..240 {
            let s = src.next().unwrap();
            assert!(
                s >= 0.40,
                "sample {i} during early fade-out should stay >= 0.40, got {s}"
            );
        }

        // After the full 20 ms fade-out, output should be at silence.
        for _ in 0..2000 {
            let _ = src.next();
        }
        let after_fade = src.next().unwrap();
        assert!(
            after_fade.abs() < 1e-3,
            "after fade-out should be silence, got {after_fade}"
        );
    }
}
