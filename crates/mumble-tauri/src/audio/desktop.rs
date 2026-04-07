//! Cpal-based audio capture and mixing playback implementations.
//!
//! Bridges the OS audio subsystem (via `cpal`) to the protocol
//! library's [`AudioCapture`] trait and the [`MixingPlayback`] trait
//! so that real hardware can be driven by the mixer infrastructure.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tracing::{error, warn};

use mumble_protocol::audio::capture::AudioCapture;
use mumble_protocol::audio::sample::{AudioFormat, AudioFrame};
use mumble_protocol::error::{Error, Result};

// -- Capture --------------------------------------------------------

/// Captures microphone input via cpal and makes it available as
/// [`AudioFrame`]s through the [`AudioCapture`] trait.
///
/// Internally a cpal input stream pushes samples into a lock-based
/// ring buffer. [`read_frame`](AudioCapture::read_frame) drains
/// exactly one frame's worth of samples (960 @ 48 kHz = 20 ms).
pub struct CpalCapture {
    format: AudioFormat,
    /// Samples per channel per frame (e.g. 960 for 20 ms @ 48 kHz).
    frame_size: usize,
    sequence: u64,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    stream: Option<cpal::Stream>,
    device: cpal::Device,
    /// Number of channels the hardware actually uses.
    hw_channels: u16,
    /// Live input volume multiplier (`f32` bits in `AtomicU32`).
    volume: Arc<AtomicU32>,
    /// Suppresses repeated overflow warnings from the cpal callback.
    /// Set to `true` on first overflow, cleared when the consumer
    /// catches up in `read_frame`.
    overflow_warned: Arc<AtomicBool>,
}

// SAFETY: On Windows / WASAPI the underlying COM objects use the MTA
// model and are safe to send between threads.  The `!Send` marker in
// cpal is a conservative cross-platform guard that does not apply here.
#[allow(unsafe_code, reason = "WASAPI COM objects are MTA-safe; cpal's !Send is a conservative cross-platform guard")]
unsafe impl Send for CpalCapture {}

impl CpalCapture {
    /// Create a new capture source.
    ///
    /// * `device_name` - choose a specific device, or `None` for default.
    /// * `frame_size` - samples per channel per frame (480 for Mumble).
    /// * `volume` - shared atomic volume multiplier (f32 bits as u32).
    pub fn new(device_name: Option<&str>, frame_size: usize, volume: Arc<AtomicU32>) -> Result<Self> {
        let host = cpal::default_host();

        let device = if let Some(name) = device_name {
            host.input_devices()
                .map_err(|e| Error::InvalidState(e.to_string()))?
                .find(|d| {
                    d.description()
                        .ok()
                        .map(|desc| desc.name().to_string())
                        .as_deref()
                        == Some(name)
                })
                .ok_or_else(|| {
                    Error::InvalidState(format!("Input device '{name}' not found"))
                })?
        } else {
            host.default_input_device()
                .ok_or_else(|| Error::InvalidState("No default input device".into()))?
        };

        // Use the device's preferred channel count so we don't fail on
        // devices that only support stereo.
        let default_cfg = device
            .default_input_config()
            .map_err(|e| Error::InvalidState(e.to_string()))?;
        let hw_channels = default_cfg.channels();

        Ok(Self {
            format: AudioFormat::MONO_48KHZ_F32,
            frame_size,
            sequence: 0,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(9_600))),
            stream: None,
            device,
            hw_channels,
            volume,
            overflow_warned: Arc::new(AtomicBool::new(false)),
        })
    }
}

impl AudioCapture for CpalCapture {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn read_frame(&mut self) -> Result<AudioFrame> {
        let mut buf = self
            .buffer
            .lock()
            .map_err(|e| Error::InvalidState(e.to_string()))?;

        if buf.len() < self.frame_size {
            return Err(Error::NotEnoughSamples);
        }

        // If the buffer has accumulated significantly more than one
        // frame, the encoding loop fell behind.  Skip old audio to
        // avoid sending stale voice packets.
        let max_queued = self.frame_size * 5; // ~100 ms at 48 kHz
        if buf.len() > max_queued {
            let excess = buf.len() - self.frame_size;
            warn!(
                "capture buffer overflow: {} samples ({:.0} ms), dropping {} stale samples",
                buf.len(),
                buf.len() as f32 / 48.0,
                excess,
            );
            let _ = buf.drain(..excess);
        }

        self.overflow_warned.store(false, Ordering::Relaxed);

        let vol = f32::from_bits(self.volume.load(Ordering::Relaxed));
        let samples: Vec<f32> = buf
            .drain(..self.frame_size)
            .map(|s| s * vol)
            .collect();
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|s| s.to_ne_bytes())
            .collect();

        self.sequence += 1;
        Ok(AudioFrame {
            data,
            format: self.format,
            sequence: self.sequence,
            is_silent: false,
        })
    }

    fn start(&mut self) -> Result<()> {
        let buffer = self.buffer.clone();
        let hw_channels = self.hw_channels;
        let overflow_warned = self.overflow_warned.clone();

        let stream_config = cpal::StreamConfig {
            channels: hw_channels,
            sample_rate: 48_000,
            buffer_size: cpal::BufferSize::Default,
        };

        let stream = self
            .device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut buf) = buffer.lock() {
                        if hw_channels == 1 {
                            buf.extend(data.iter().copied());
                        } else {
                            // Down-mix to mono.
                            for chunk in data.chunks(hw_channels as usize) {
                                let sum: f32 = chunk.iter().sum();
                                buf.push_back(sum / hw_channels as f32);
                            }
                        }
                        // Cap at ~200 ms (9 600 samples at 48 kHz) to
                        // avoid accumulating stale audio when the
                        // encoding loop is throttled.
                        if buf.len() > 9_600 {
                            if !overflow_warned.swap(true, Ordering::Relaxed) {
                                warn!("cpal capture buffer overflow, discarding oldest samples");
                            }
                            let excess = buf.len() - 9_600;
                            let _ = buf.drain(..excess);
                        }
                    }
                },
                |err| error!("cpal input error: {err}"),
                None,
            )
            .map_err(|e| Error::InvalidState(e.to_string()))?;

        stream
            .play()
            .map_err(|e| Error::InvalidState(e.to_string()))?;

        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.stream = None;
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
        Ok(())
    }
}

// -- Factory -------------------------------------------------------

/// Desktop audio device factory backed by cpal.
pub struct CpalAudioFactory;

impl super::AudioDeviceFactory for CpalAudioFactory {
    fn create_capture(
        device_name: Option<&str>,
        frame_size: usize,
        volume: Arc<AtomicU32>,
    ) -> std::result::Result<Box<dyn AudioCapture>, String> {
        CpalCapture::new(device_name, frame_size, volume)
            .map(|c| Box::new(c) as _)
            .map_err(|e| format!("Capture init: {e}"))
    }

    fn create_mixing_playback(
        device_name: Option<&str>,
        volume: Arc<AtomicU32>,
        buffers: mumble_protocol::audio::mixer::SpeakerBuffers,
        speaker_volumes: mumble_protocol::audio::mixer::SpeakerVolumes,
    ) -> std::result::Result<Box<dyn super::MixingPlayback>, String> {
        CpalMixingPlayback::new(device_name, volume, buffers, speaker_volumes)
            .map(|c| Box::new(c) as _)
            .map_err(|e| format!("Mixing playback init: {e}"))
    }
}

// -- Mixing playback -----------------------------------------------

/// Batch-drain up to `mono_needed` samples from every active speaker
/// into `mixed_buf` (summed/mixed), then remove empty speaker entries.
///
/// Per-speaker volume is applied from `speaker_vols` (0.0-2.0,
/// defaulting to 1.0 when absent).
///
/// Returns `true` when at least one speaker provided samples.
/// The caller must hold the mutex for this call only; the lock is
/// released immediately afterwards so `mixer.feed()` is never blocked
/// during the output fill phase.
fn batch_drain_speakers(
    bufs: &mut HashMap<u32, VecDeque<f32>>,
    speaker_vols: &HashMap<u32, f32>,
    mixed_buf: &mut Vec<f32>,
    mono_needed: usize,
) -> bool {
    mixed_buf.clear();
    mixed_buf.resize(mono_needed, 0.0_f32);
    let mut any = false;

    bufs.retain(|session, buf| {
        if buf.is_empty() {
            return false;
        }
        any = true;
        let vol = speaker_vols.get(session).copied().unwrap_or(1.0);
        let n = buf.len().min(mono_needed);
        let (a, b) = buf.as_slices();
        let from_a = n.min(a.len());
        for (dst, src) in mixed_buf[..from_a].iter_mut().zip(&a[..from_a]) {
            *dst += *src * vol;
        }
        if from_a < n {
            let from_b = n - from_a;
            for (dst, src) in mixed_buf[from_a..n].iter_mut().zip(&b[..from_b]) {
                *dst += *src * vol;
            }
        }
        let _ = buf.drain(..n);
        !buf.is_empty()
    });

    any
}

/// Multi-speaker mixing playback backed by cpal.
///
/// Instead of receiving frames via `write_frame`, this device reads
/// decoded samples directly from per-speaker ring buffers (managed by
/// [`AudioMixer`](mumble_protocol::audio::mixer::AudioMixer)) and sums
/// them in the cpal output callback.
pub struct CpalMixingPlayback {
    stream: Option<cpal::Stream>,
    device: cpal::Device,
    volume: Arc<AtomicU32>,
    buffers: mumble_protocol::audio::mixer::SpeakerBuffers,
    speaker_volumes: mumble_protocol::audio::mixer::SpeakerVolumes,
}

// SAFETY: See CpalCapture.
#[allow(unsafe_code, reason = "WASAPI COM objects are MTA-safe; cpal's !Send is a conservative cross-platform guard")]
unsafe impl Send for CpalMixingPlayback {}

impl CpalMixingPlayback {
    pub fn new(
        device_name: Option<&str>,
        volume: Arc<AtomicU32>,
        buffers: mumble_protocol::audio::mixer::SpeakerBuffers,
        speaker_volumes: mumble_protocol::audio::mixer::SpeakerVolumes,
    ) -> Result<Self> {
        let host = cpal::default_host();
        let device = if let Some(name) = device_name {
            host.output_devices()
                .map_err(|e| Error::InvalidState(e.to_string()))?
                .find(|d| {
                    d.description()
                        .ok()
                        .map(|desc| desc.name() == name)
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    Error::InvalidState(format!("Output device not found: {name}"))
                })?
        } else {
            host.default_output_device()
                .ok_or_else(|| Error::InvalidState("No default output device".into()))?
        };

        Ok(Self {
            stream: None,
            device,
            volume,
            buffers,
            speaker_volumes,
        })
    }
}

impl super::MixingPlayback for CpalMixingPlayback {
    fn start(&mut self) -> Result<()> {
        let buffers = self.buffers.clone();
        let volume = self.volume.clone();
        let speaker_volumes = self.speaker_volumes.clone();

        // Pre-buffer: wait until at least one speaker has 60 ms
        // of decoded audio before starting output, to absorb
        // network jitter and prevent pops.
        const PRE_BUFFER_SAMPLES: usize = 2880; // 60 ms @ 48 kHz
        let primed = Arc::new(AtomicBool::new(false));
        let primed_cb = primed.clone();

        let stream_config = cpal::StreamConfig {
            channels: 2,
            sample_rate: 48_000,
            buffer_size: cpal::BufferSize::Default,
        };

        let mut last_sample: f32 = 0.0;
        let mut in_underrun = false;
        let mut ramp_pos: usize = 0;
        let mut underrun_samples: usize = 0;
        const RAMP_SAMPLES: usize = 96;
        const DECAY: f32 = 0.99;
        const REPRIME_THRESHOLD: usize = 9600;

        // Pre-mixed mono buffer reused across callbacks to avoid
        // repeated allocation.
        let mut mixed_buf: Vec<f32> = Vec::new();

        let stream = self
            .device
            .build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let vol = f32::from_bits(volume.load(Ordering::Relaxed));
                    let mono_needed = data.len() / 2;

                    // -- Critical section: hold the mutex only for the
                    //    batch drain, then release immediately so that
                    //    mixer.feed() on the network thread is never
                    //    blocked for the duration of the output fill. --
                    let drained = {
                        let Ok(mut bufs) = buffers.lock() else {
                            data.fill(0.0);
                            return;
                        };

                        // Pre-buffer: wait for enough data before playing.
                        if !primed_cb.load(Ordering::Relaxed) {
                            let max_available: usize =
                                bufs.values().map(VecDeque::len).max().unwrap_or(0);
                            if max_available < PRE_BUFFER_SAMPLES {
                                data.fill(0.0);
                                return;
                            }
                            primed_cb.store(true, Ordering::Relaxed);
                        }

                        // Snapshot per-speaker volumes (separate lock).
                        let sv = speaker_volumes.lock()
                            .map(|g| g.clone())
                            .unwrap_or_default();

                        batch_drain_speakers(&mut bufs, &sv, &mut mixed_buf, mono_needed)
                        // mutex released here
                    };

                    // -- Write interleaved stereo from the local mixed
                    //    buffer without holding any lock. --
                    for (i, chunk) in data.chunks_exact_mut(2).enumerate() {
                        let has_data = drained && i < mixed_buf.len();

                        if has_data {
                            let sample = mixed_buf[i].tanh();

                            let out = if in_underrun {
                                ramp_pos += 1;
                                if ramp_pos >= RAMP_SAMPLES {
                                    in_underrun = false;
                                    ramp_pos = 0;
                                    underrun_samples = 0;
                                    sample
                                } else {
                                    let t = ramp_pos as f32 / RAMP_SAMPLES as f32;
                                    let gain = 0.5
                                        * (1.0 - (std::f32::consts::PI * t).cos());
                                    last_sample * (1.0 - gain) + sample * gain
                                }
                            } else {
                                sample
                            };
                            last_sample = out;
                            chunk[0] = out * vol;
                            chunk[1] = out * vol;
                        } else {
                            in_underrun = true;
                            ramp_pos = 0;
                            underrun_samples += 1;
                            last_sample *= DECAY;
                            if last_sample.abs() < 1e-6 {
                                last_sample = 0.0;
                            }
                            if underrun_samples >= REPRIME_THRESHOLD {
                                primed_cb.store(false, Ordering::Relaxed);
                            }
                            chunk[0] = last_sample * vol;
                            chunk[1] = last_sample * vol;
                        }
                    }
                },
                |err| error!("cpal mixing output error: {err}"),
                None,
            )
            .map_err(|e| Error::InvalidState(e.to_string()))?;

        stream
            .play()
            .map_err(|e| Error::InvalidState(e.to_string()))?;

        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.stream = None;
        if let Ok(mut bufs) = self.buffers.lock() {
            bufs.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, unused_results, reason = "acceptable in test code")]
    use super::*;
    use mumble_protocol::audio::sample::AudioFormat;

    #[test]
    #[ignore = "requires audio hardware - run with --ignored"]
    fn capture_reports_mono_48khz_f32_format() {
        let vol = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
        let Ok(capture) = CpalCapture::new(None, 960, vol) else {
            eprintln!("Skipping: no audio input device available");
            return;
        };
        assert_eq!(capture.format(), AudioFormat::MONO_48KHZ_F32);
    }

    #[test]
    #[ignore = "requires audio hardware - run with --ignored"]
    fn mixing_playback_can_be_created() {
        let vol = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
        let bufs: mumble_protocol::audio::mixer::SpeakerBuffers = Arc::new(
            Mutex::new(HashMap::new()),
        );
        let svols: mumble_protocol::audio::mixer::SpeakerVolumes =
            Arc::new(Mutex::new(HashMap::new()));
        let Ok(_p) = CpalMixingPlayback::new(None, vol, bufs, svols) else {
            eprintln!("Skipping: no audio output device available");
            return;
        };
    }

    #[test]
    #[ignore = "requires audio hardware - run with --ignored"]
    fn capture_read_before_start_returns_error() {
        let vol = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
        let Ok(mut capture) = CpalCapture::new(None, 960, vol) else { return };
        // Without calling start(), the buffer is empty so read_frame should fail.
        assert!(capture.read_frame().is_err());
    }

    #[test]
    fn batch_drain_sums_multiple_speakers() {
        let mut bufs = HashMap::new();
        bufs.insert(1u32, VecDeque::from(vec![0.5_f32; 10]));
        bufs.insert(2, VecDeque::from(vec![0.25; 10]));
        let speaker_vols: HashMap<u32, f32> = HashMap::new();
        let mut mixed = Vec::new();

        let had = batch_drain_speakers(&mut bufs, &speaker_vols, &mut mixed, 10);
        assert!(had);
        assert_eq!(mixed.len(), 10);
        for &s in &mixed {
            assert!((s - 0.75).abs() < 1e-6, "expected 0.75, got {s}");
        }
        // Both speakers fully drained, so entries removed.
        assert!(bufs.is_empty());
    }

    #[test]
    fn batch_drain_partial_speaker() {
        let mut bufs = HashMap::new();
        bufs.insert(1u32, VecDeque::from(vec![1.0_f32; 5]));
        let speaker_vols: HashMap<u32, f32> = HashMap::new();
        let mut mixed = Vec::new();

        let had = batch_drain_speakers(&mut bufs, &speaker_vols, &mut mixed, 10);
        assert!(had);
        // First 5 samples mixed, rest zero-filled.
        assert_eq!(mixed.len(), 10);
        for &s in &mixed[..5] {
            assert!((s - 1.0).abs() < 1e-6);
        }
        for &s in &mixed[5..] {
            assert!(s.abs() < 1e-6);
        }
    }

    #[test]
    fn batch_drain_empty_returns_false() {
        let mut bufs: HashMap<u32, VecDeque<f32>> = HashMap::new();
        let speaker_vols: HashMap<u32, f32> = HashMap::new();
        let mut mixed = Vec::new();
        assert!(!batch_drain_speakers(&mut bufs, &speaker_vols, &mut mixed, 10));
    }

    #[test]
    fn batch_drain_retains_leftover_samples() {
        let mut bufs = HashMap::new();
        bufs.insert(1u32, VecDeque::from(vec![0.5_f32; 20]));
        let speaker_vols: HashMap<u32, f32> = HashMap::new();
        let mut mixed = Vec::new();

        batch_drain_speakers(&mut bufs, &speaker_vols, &mut mixed, 10);
        // 10 drained, 10 remain.
        assert_eq!(bufs[&1].len(), 10);
    }

    #[test]
    fn batch_drain_applies_per_speaker_volume() {
        let mut bufs = HashMap::new();
        bufs.insert(1u32, VecDeque::from(vec![1.0_f32; 4]));
        bufs.insert(2u32, VecDeque::from(vec![1.0_f32; 4]));

        let mut speaker_vols = HashMap::new();
        speaker_vols.insert(1u32, 0.5_f32); // speaker 1 at 50%
        speaker_vols.insert(2u32, 1.5_f32); // speaker 2 at 150%

        let mut mixed = Vec::new();
        let had = batch_drain_speakers(&mut bufs, &speaker_vols, &mut mixed, 4);
        assert!(had);
        // Each sample = 1.0*0.5 + 1.0*1.5 = 2.0
        for &s in &mixed {
            assert!((s - 2.0).abs() < 1e-6, "expected 2.0, got {s}");
        }
    }

    #[test]
    fn batch_drain_default_volume_is_unity() {
        let mut bufs = HashMap::new();
        bufs.insert(1u32, VecDeque::from(vec![0.8_f32; 4]));

        // No entry for speaker 1 means default volume (1.0)
        let speaker_vols: HashMap<u32, f32> = HashMap::new();
        let mut mixed = Vec::new();
        batch_drain_speakers(&mut bufs, &speaker_vols, &mut mixed, 4);

        for &s in &mixed {
            assert!((s - 0.8).abs() < 1e-6, "expected 0.8 (unity), got {s}");
        }
    }
}
