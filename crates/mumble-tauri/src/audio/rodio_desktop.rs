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
/// Consecutive empty chunk-level refills before repriming (~1.5 s).
/// Each empty refill represents one `MIX_CHUNK_SIZE` period (20 ms).
const REPRIME_AFTER: u32 = 75;

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
    /// Remaining samples to skip before the next `refill_chunk()` call.
    /// Prevents per-sample refill attempts during underrun, throttling
    /// them to once per chunk (~20 ms) so `consecutive_empty` counts
    /// as chunk-level events rather than sample-level.
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
            self.underrun_cooldown = MIX_CHUNK_SIZE;
            return;
        };

        if !self.primed {
            let max_available = bufs.values().map(VecDeque::len).max().unwrap_or(0);
            if max_available < PRE_BUFFER_SAMPLES {
                self.chunk_pos = 0;
                self.chunk_valid = 0;
                self.underrun_cooldown = MIX_CHUNK_SIZE;
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
            self.underrun_cooldown = MIX_CHUNK_SIZE;
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
                const MAX_RAMP: usize = 48;
                let ramp_len = self.underrun_samples.clamp(8, MAX_RAMP);
                self.ramp_pos += 1;
                if self.ramp_pos >= ramp_len {
                    self.in_underrun = false;
                    self.ramp_pos = 0;
                    self.underrun_samples = 0;
                    raw
                } else {
                    let t = self.ramp_pos as f32 / ramp_len as f32;
                    self.last_sample * (1.0 - t) + raw * t
                }
            } else {
                raw
            };

            self.diag.peak = self.diag.peak.max(sample.abs());
            self.last_sample = sample;
            Some(super::soft_clip(sample * vol))
        } else {
            // Underrun: exponential decay of last sample instead of
            // hard silence jump which causes audible clicks.
            const DECAY: f32 = 0.997;
            self.in_underrun = true;
            self.ramp_pos = 0;
            self.underrun_samples += 1;
            self.last_sample *= DECAY;
            if self.last_sample.abs() < 1e-6 {
                self.last_sample = 0.0;
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

        // Next samples are underrun: should NOT jump to 0.0 but decay
        // smoothly from the last sample.
        let first_underrun = src.next().unwrap();
        assert!(
            first_underrun.abs() > 0.3,
            "first underrun sample should decay from ~0.5, got {first_underrun}"
        );

        // After many underrun samples, should settle near zero.
        for _ in 0..5000 {
            let _ = src.next();
        }
        let late_underrun = src.next().unwrap();
        assert!(
            late_underrun.abs() < 0.01,
            "late underrun should be near zero, got {late_underrun}"
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
        // until the next refill attempt at the chunk boundary.
        let remaining = src.underrun_cooldown;
        for _ in 0..remaining {
            let _ = src.next();
        }

        // First samples after resume should ramp from decayed last_sample
        // toward -0.3, NOT jump directly to -0.3.
        let resume_sample = src.next().unwrap();
        // The ramp blends last_sample (decayed ~0.49) toward -0.3.
        // First ramp sample should be closer to last_sample than to -0.3.
        assert!(
            resume_sample > -0.2,
            "resume should ramp, not jump to -0.3; got {resume_sample}"
        );

        // After the full ramp completes, samples should be ~-0.3.
        for _ in 0..50 {
            let _ = src.next();
        }
        let settled = src.next().unwrap();
        assert!(
            (settled - (-0.3)).abs() < 0.05,
            "after ramp should settle to -0.3, got {settled}"
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

        // Drain one full chunk of underrun (960 samples).
        // With the cooldown fix, this should count as exactly 1
        // consecutive_empty instead of 960.
        for _ in 0..MIX_CHUNK_SIZE {
            let _ = src.next();
        }

        assert!(src.primed, "should still be primed after a single-chunk underrun");
        assert_eq!(
            src.consecutive_empty, 1,
            "one chunk of underrun should count as 1 empty refill, not {}", src.consecutive_empty
        );
        assert_eq!(src.diag.reprime_count, 0, "no repriming should have occurred");
    }
}
