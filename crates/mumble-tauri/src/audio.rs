//! Cpal-based audio capture and playback implementations.
//!
//! Bridges the OS audio subsystem (via `cpal`) to the protocol
//! library's [`AudioCapture`] / [`AudioPlayback`] traits so that
//! the existing pipeline infrastructure can drive real hardware.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tracing::{error, warn};

use mumble_protocol::audio::capture::AudioCapture;
use mumble_protocol::audio::playback::AudioPlayback;
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
#[allow(unsafe_code)]
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
                                buf.pop_front();
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

// -- Playback -------------------------------------------------------

/// Plays decoded PCM audio through the default output device.
///
/// [`write_frame`](AudioPlayback::write_frame) pushes mono F32
/// samples into a shared buffer that the cpal output callback
/// drains, duplicating mono -> stereo for the hardware.
pub struct CpalPlayback {
    format: AudioFormat,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    stream: Option<cpal::Stream>,
    device: cpal::Device,
    /// Live output volume multiplier (`f32` bits in `AtomicU32`).
    volume: Arc<AtomicU32>,
}

// SAFETY: See `CpalCapture` - same justification applies.
#[allow(unsafe_code)]
unsafe impl Send for CpalPlayback {}

impl CpalPlayback {
    pub fn new(device_name: Option<&str>, volume: Arc<AtomicU32>) -> Result<Self> {
        use cpal::traits::{DeviceTrait, HostTrait};

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
            format: AudioFormat::MONO_48KHZ_F32,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(96_000))),
            stream: None,
            device,
            volume,
        })
    }
}

impl AudioPlayback for CpalPlayback {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn write_frame(&mut self, frame: &AudioFrame) -> Result<()> {
        let samples = frame.as_f32_samples();
        let mut buf = self
            .buffer
            .lock()
            .map_err(|e| Error::InvalidState(e.to_string()))?;
        buf.extend(samples.iter().copied());
        // Cap at ~2 seconds.
        if buf.len() > 96_000 {
            warn!("cpal playback buffer overflow, discarding oldest samples");
            while buf.len() > 96_000 {
                buf.pop_front();
            }
        }
        Ok(())
    }

    fn start(&mut self) -> Result<()> {
        let buffer = self.buffer.clone();
        let volume = self.volume.clone();

        // Pre-buffer: accumulate samples before starting playback to
        // absorb network jitter and prevent pops at stream start.
        // 60 ms @ 48 kHz mono = 2880 samples.
        const PRE_BUFFER_SAMPLES: usize = 2880;
        let primed = Arc::new(AtomicBool::new(false));
        let primed_cb = primed.clone();
        let volume_cb = volume;

        // Most output devices are stereo; duplicate mono to both channels.
        let stream_config = cpal::StreamConfig {
            channels: 2,
            sample_rate: 48_000,
            buffer_size: cpal::BufferSize::Default,
        };

        // Mutable state owned by the callback closure for smooth
        // underrun handling: exponential decay on underrun instead of
        // hard silence, and a raised-cosine crossfade ramp on recovery.
        let mut last_sample: f32 = 0.0;
        let mut in_underrun = false;
        let mut ramp_pos: usize = 0;
        let mut underrun_samples: usize = 0;
        // 2 ms crossfade at 48 kHz - long enough to avoid clicks,
        // short enough to avoid smearing transients.
        const RAMP_SAMPLES: usize = 96;
        // Gentle decay: reaches -60 dB in ~8 ms (vs 2 ms at 0.95).
        // This reduces harmonic distortion from non-linear processing
        // while still preventing hard-edge pops.
        const DECAY: f32 = 0.99;
        // If underrun lasts longer than 200 ms (9600 samples), re-prime
        // the pre-buffer so the next burst plays smoothly.  This only
        // triggers for extended silence (e.g. end of speech), not for
        // brief network hiccups.
        const REPRIME_THRESHOLD: usize = 9600;

        let stream = self
            .device
            .build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let Ok(mut buf) = buffer.lock() else {
                        data.fill(0.0);
                        return;
                    };

                    // Read volume once per callback for consistency.
                    let vol = f32::from_bits(volume_cb.load(Ordering::Relaxed));

                    // Wait for pre-buffer level before starting playback.
                    if !primed_cb.load(Ordering::Relaxed) {
                        if buf.len() < PRE_BUFFER_SAMPLES {
                            data.fill(0.0);
                            return;
                        }
                        primed_cb.store(true, Ordering::Relaxed);
                    }

                    // Fill interleaved stereo: each mono sample -> L + R.
                    for chunk in data.chunks_exact_mut(2) {
                        if let Some(sample) = buf.pop_front() {
                            let out = if in_underrun {
                                // Recovering: raised-cosine crossfade from
                                // decayed value to incoming signal.
                                ramp_pos += 1;
                                if ramp_pos >= RAMP_SAMPLES {
                                    in_underrun = false;
                                    ramp_pos = 0;
                                    underrun_samples = 0;
                                    sample
                                } else {
                                    let t = ramp_pos as f32 / RAMP_SAMPLES as f32;
                                    let gain =
                                        0.5 * (1.0 - (std::f32::consts::PI * t).cos());
                                    last_sample * (1.0 - gain) + sample * gain
                                }
                            } else {
                                sample
                            };
                            last_sample = out;
                            chunk[0] = out * vol;
                            chunk[1] = out * vol;
                        } else {
                            // Buffer empty: smooth fade-out via
                            // exponential decay to avoid pop.
                            in_underrun = true;
                            ramp_pos = 0;
                            underrun_samples += 1;
                            last_sample *= DECAY;
                            if last_sample.abs() < 1e-6 {
                                last_sample = 0.0;
                            }

                            // Extended underrun: re-prime so the next
                            // burst of audio buffers up before playing.
                            if underrun_samples >= REPRIME_THRESHOLD {
                                primed_cb.store(false, Ordering::Relaxed);
                            }

                            chunk[0] = last_sample * vol;
                            chunk[1] = last_sample * vol;
                        }
                    }
                },
                |err| error!("cpal output error: {err}"),
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
