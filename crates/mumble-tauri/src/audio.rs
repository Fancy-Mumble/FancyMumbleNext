//! Cpal-based audio capture and playback implementations.
//!
//! Bridges the OS audio subsystem (via `cpal`) to the protocol
//! library's [`AudioCapture`] / [`AudioPlayback`] traits so that
//! the existing pipeline infrastructure can drive real hardware.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tracing::error;

use mumble_protocol::audio::capture::AudioCapture;
use mumble_protocol::audio::playback::AudioPlayback;
use mumble_protocol::audio::sample::{AudioFormat, AudioFrame};
use mumble_protocol::error::{Error, Result};

// ── Capture ────────────────────────────────────────────────────────

/// Captures microphone input via cpal and makes it available as
/// [`AudioFrame`]s through the [`AudioCapture`] trait.
///
/// Internally a cpal input stream pushes samples into a lock-based
/// ring buffer. [`read_frame`](AudioCapture::read_frame) drains
/// exactly one frame's worth of samples (480 @ 48 kHz = 10 ms).
pub struct CpalCapture {
    format: AudioFormat,
    /// Samples per channel per frame (e.g. 480 for 10 ms @ 48 kHz).
    frame_size: usize,
    sequence: u64,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    stream: Option<cpal::Stream>,
    device: cpal::Device,
    /// Number of channels the hardware actually uses.
    hw_channels: u16,
}

// SAFETY: On Windows / WASAPI the underlying COM objects use the MTA
// model and are safe to send between threads.  The `!Send` marker in
// cpal is a conservative cross-platform guard that does not apply here.
#[allow(unsafe_code)]
unsafe impl Send for CpalCapture {}

impl CpalCapture {
    /// Create a new capture source.
    ///
    /// * `device_name` – choose a specific device, or `None` for default.
    /// * `frame_size` – samples per channel per frame (480 for Mumble).
    pub fn new(device_name: Option<&str>, frame_size: usize) -> Result<Self> {
        let host = cpal::default_host();

        let device = if let Some(name) = device_name {
            host.input_devices()
                .map_err(|e| Error::InvalidState(e.to_string()))?
                .find(|d| d.name().ok().as_deref() == Some(name))
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
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(48_000))),
            stream: None,
            device,
            hw_channels,
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
            return Err(Error::InvalidState("Not enough samples".into()));
        }

        let samples: Vec<f32> = buf.drain(..self.frame_size).collect();
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
            sample_rate: cpal::SampleRate(48_000),
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
                        // Cap at ~1 second to prevent unbounded growth.
                        while buf.len() > 48_000 {
                            buf.pop_front();
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

// ── Playback ───────────────────────────────────────────────────────

/// Plays decoded PCM audio through the default output device.
///
/// [`write_frame`](AudioPlayback::write_frame) pushes mono F32
/// samples into a shared buffer that the cpal output callback
/// drains, duplicating mono → stereo for the hardware.
pub struct CpalPlayback {
    format: AudioFormat,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    stream: Option<cpal::Stream>,
    device: cpal::Device,
}

// SAFETY: See `CpalCapture` - same justification applies.
#[allow(unsafe_code)]
unsafe impl Send for CpalPlayback {}

impl CpalPlayback {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| Error::InvalidState("No default output device".into()))?;

        Ok(Self {
            format: AudioFormat::MONO_48KHZ_F32,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(48_000))),
            stream: None,
            device,
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
        // Cap at ~1 second.
        while buf.len() > 48_000 {
            buf.pop_front();
        }
        Ok(())
    }

    fn start(&mut self) -> Result<()> {
        let buffer = self.buffer.clone();

        // Most output devices are stereo; duplicate mono to both channels.
        let stream_config = cpal::StreamConfig {
            channels: 2,
            sample_rate: cpal::SampleRate(48_000),
            buffer_size: cpal::BufferSize::Default,
        };

        let stream = self
            .device
            .build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let Ok(mut buf) = buffer.lock() else {
                        data.fill(0.0);
                        return;
                    };
                    // Fill interleaved stereo: each mono sample → L + R.
                    for chunk in data.chunks_exact_mut(2) {
                        let sample = buf.pop_front().unwrap_or(0.0);
                        chunk[0] = sample;
                        chunk[1] = sample;
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
