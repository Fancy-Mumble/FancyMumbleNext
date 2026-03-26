//! Cpal-based audio capture and mixing playback implementations.
//!
//! Bridges the OS audio subsystem (via `cpal`) to the protocol
//! library's [`AudioCapture`] trait and the [`MixingPlayback`] trait
//! so that real hardware can be driven by the mixer infrastructure.

use std::collections::VecDeque;
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
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(96_000))),
            stream: None,
            device,
            hw_channels,
            volume,
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
                        // Cap at ~2 seconds to prevent unbounded growth.
                        if buf.len() > 96_000 {
                            warn!("cpal capture buffer overflow, discarding oldest samples");
                            while buf.len() > 96_000 {
                                let _ = buf.pop_front();
                            }
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
    ) -> std::result::Result<Box<dyn super::MixingPlayback>, String> {
        CpalMixingPlayback::new(device_name, volume, buffers)
            .map(|c| Box::new(c) as _)
            .map_err(|e| format!("Mixing playback init: {e}"))
    }
}

// -- Mixing playback -----------------------------------------------

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
}

// SAFETY: See CpalCapture.
#[allow(unsafe_code, reason = "WASAPI COM objects are MTA-safe; cpal's !Send is a conservative cross-platform guard")]
unsafe impl Send for CpalMixingPlayback {}

impl CpalMixingPlayback {
    pub fn new(
        device_name: Option<&str>,
        volume: Arc<AtomicU32>,
        buffers: mumble_protocol::audio::mixer::SpeakerBuffers,
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
        })
    }
}

impl super::MixingPlayback for CpalMixingPlayback {
    fn start(&mut self) -> Result<()> {
        let buffers = self.buffers.clone();
        let volume = self.volume.clone();

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

        let stream = self
            .device
            .build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let vol = f32::from_bits(volume.load(Ordering::Relaxed));

                    let Ok(mut bufs) = buffers.lock() else {
                        data.fill(0.0);
                        return;
                    };

                    // Pre-buffer: wait for enough data before playing.
                    if !primed_cb.load(Ordering::Relaxed) {
                        let total_available: usize =
                            bufs.values().map(VecDeque::len).max().unwrap_or(0);
                        if total_available < PRE_BUFFER_SAMPLES {
                            data.fill(0.0);
                            return;
                        }
                        primed_cb.store(true, Ordering::Relaxed);
                    }

                    // Fill interleaved stereo output.
                    for chunk in data.chunks_exact_mut(2) {
                        // Sum one sample from each active speaker.
                        let mut mixed: f32 = 0.0;
                        let mut had_sample = false;
                        for buf in bufs.values_mut() {
                            if let Some(s) = buf.pop_front() {
                                mixed += s;
                                had_sample = true;
                            }
                        }

                        if had_sample {
                            // Soft-clip: tanh prevents harsh clipping when
                            // multiple loud speakers overlap.
                            let sample = mixed.tanh();

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
                            // All speaker buffers empty: smooth decay.
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

                    // Clean up empty speaker buffers to avoid accumulating
                    // stale entries from speakers who stopped talking.
                    bufs.retain(|_, b| !b.is_empty());
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
            Mutex::new(std::collections::HashMap::new()),
        );
        let Ok(_p) = CpalMixingPlayback::new(None, vol, bufs) else {
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
}
