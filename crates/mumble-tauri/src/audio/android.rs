//! Oboe-based audio capture and mixing playback for Android.
//!
//! Mirrors the desktop cpal implementation: callbacks push/pull samples
//! through shared ring buffers, while the mixer drives speaker-level I/O
//! via the [`AudioCapture`] and [`MixingPlayback`](super::MixingPlayback) traits.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use oboe::{
    AudioInputCallback, AudioOutputCallback, AudioOutputStreamSafe, AudioStream,
    AudioStreamAsync, AudioStreamBuilder, ContentType, DataCallbackResult, Input,
    InputPreset, Mono, Output, PerformanceMode, SessionId, SharingMode, Usage,
};
use tracing::{error, warn};

use mumble_protocol::audio::capture::AudioCapture;
use mumble_protocol::audio::sample::{AudioFormat, AudioFrame};
use mumble_protocol::error::{Error, Result};

// -- Capture --------------------------------------------------------

/// Callback struct that receives microphone samples from oboe and
/// pushes them into a shared ring buffer.
struct CaptureCallback {
    buffer: Arc<Mutex<VecDeque<f32>>>,
}

impl AudioInputCallback for CaptureCallback {
    type FrameType = (f32, Mono);

    fn on_audio_ready(
        &mut self,
        _stream: &mut dyn oboe::AudioInputStreamSafe,
        frames: &[f32],
    ) -> DataCallbackResult {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.extend(frames.iter().copied());
            // Cap at ~2 seconds to prevent unbounded growth.
            if buf.len() > 96_000 {
                warn!("oboe capture buffer overflow, discarding oldest samples");
                while buf.len() > 96_000 {
                    let _ = buf.pop_front();
                }
            }
        }
        DataCallbackResult::Continue
    }

    fn on_error_before_close(
        &mut self,
        _stream: &mut dyn oboe::AudioInputStreamSafe,
        e: oboe::Error,
    ) {
        error!("oboe capture error: {e:?}");
    }
}

/// Captures microphone input via oboe (Android) and makes it available
/// as [`AudioFrame`]s through the [`AudioCapture`] trait.
pub struct OboeCapture {
    format: AudioFormat,
    frame_size: usize,
    sequence: u64,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    stream: Option<AudioStreamAsync<Input, CaptureCallback>>,
    volume: Arc<AtomicU32>,
}

// SAFETY: The oboe stream is only accessed from the pipeline thread
// after construction. The audio callback thread communicates solely
// through the `Arc<Mutex<VecDeque>>` buffer.
#[allow(unsafe_code, reason = "oboe stream accessed from single pipeline thread; callback uses Arc<Mutex>")]
unsafe impl Send for OboeCapture {}

impl OboeCapture {
    /// Create a new capture source for Android.
    ///
    /// * `frame_size` - samples per channel per frame (960 for Mumble 20 ms).
    /// * `volume` - shared atomic volume multiplier (f32 bits as u32).
    pub fn new(frame_size: usize, volume: Arc<AtomicU32>) -> Result<Self> {
        Ok(Self {
            format: AudioFormat::MONO_48KHZ_F32,
            frame_size,
            sequence: 0,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(96_000))),
            stream: None,
            volume,
        })
    }
}

impl AudioCapture for OboeCapture {
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
        let samples: Vec<f32> = buf.drain(..self.frame_size).map(|s| s * vol).collect();
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
        let callback = CaptureCallback {
            buffer: self.buffer.clone(),
        };

        let mut stream = AudioStreamBuilder::default()
            .set_input()
            .set_performance_mode(PerformanceMode::LowLatency)
            .set_sharing_mode(SharingMode::Shared)
            .set_format::<f32>()
            .set_channel_count::<Mono>()
            .set_sample_rate(48_000)
            .set_input_preset(InputPreset::VoiceRecognition)
            .set_session_id(SessionId::None)
            .set_callback(callback)
            .open_stream()
            .map_err(|e| Error::InvalidState(format!("oboe input open: {e:?}")))?;

        stream
            .start()
            .map_err(|e| Error::InvalidState(format!("oboe input start: {e:?}")))?;

        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(mut stream) = self.stream.take() {
            let _ = stream.stop();
        }
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
        Ok(())
    }
}

// -- Factory -------------------------------------------------------

/// Android audio device factory backed by oboe.
pub struct OboeAudioFactory;

impl super::AudioDeviceFactory for OboeAudioFactory {
    fn create_capture(
        _device_name: Option<&str>,
        frame_size: usize,
        volume: Arc<AtomicU32>,
    ) -> std::result::Result<Box<dyn AudioCapture>, String> {
        OboeCapture::new(frame_size, volume)
            .map(|c| Box::new(c) as _)
            .map_err(|e| format!("Capture init: {e}"))
    }

    fn create_mixing_playback(
        _device_name: Option<&str>,
        volume: Arc<AtomicU32>,
        buffers: mumble_protocol::audio::mixer::SpeakerBuffers,
        _speaker_volumes: mumble_protocol::audio::mixer::SpeakerVolumes,
    ) -> std::result::Result<Box<dyn super::MixingPlayback>, String> {
        OboeMixingPlayback::new(volume, buffers)
            .map(|c| Box::new(c) as _)
            .map_err(|e| format!("Mixing playback init: {e}"))
    }
}

// -- Mixing playback -----------------------------------------------

/// Callback for the mixing playback stream.
/// Reads from all per-speaker buffers, sums them, and outputs.
struct MixingPlaybackCallback {
    buffers: mumble_protocol::audio::mixer::SpeakerBuffers,
    volume: Arc<AtomicU32>,
    last_sample: f32,
}

impl AudioOutputCallback for MixingPlaybackCallback {
    type FrameType = (f32, Mono);

    fn on_audio_ready(
        &mut self,
        _stream: &mut dyn AudioOutputStreamSafe,
        frames: &mut [f32],
    ) -> DataCallbackResult {
        let vol = f32::from_bits(self.volume.load(Ordering::Relaxed));

        if let Ok(mut bufs) = self.buffers.lock() {
            for sample in frames.iter_mut() {
                let mut mixed: f32 = 0.0;
                let mut had_sample = false;
                for buf in bufs.values_mut() {
                    if let Some(s) = buf.pop_front() {
                        mixed += s;
                        had_sample = true;
                    }
                }

                if had_sample {
                    let out = mixed.tanh();
                    self.last_sample = out;
                    *sample = out * vol;
                } else {
                    self.last_sample *= 0.99;
                    if self.last_sample.abs() < 1e-6 {
                        self.last_sample = 0.0;
                    }
                    *sample = self.last_sample * vol;
                }
            }
            bufs.retain(|_, b| !b.is_empty());
        } else {
            frames.fill(0.0);
        }

        DataCallbackResult::Continue
    }

    fn on_error_before_close(
        &mut self,
        _stream: &mut dyn AudioOutputStreamSafe,
        e: oboe::Error,
    ) {
        error!("oboe mixing playback error: {e:?}");
    }
}

/// Multi-speaker mixing playback backed by oboe (Android).
pub struct OboeMixingPlayback {
    stream: Option<AudioStreamAsync<Output, MixingPlaybackCallback>>,
    volume: Arc<AtomicU32>,
    buffers: mumble_protocol::audio::mixer::SpeakerBuffers,
}

// SAFETY: Same justification as OboeCapture.
#[allow(unsafe_code, reason = "oboe stream accessed from single pipeline thread; callback uses Arc<Mutex>")]
unsafe impl Send for OboeMixingPlayback {}

impl OboeMixingPlayback {
    pub fn new(
        volume: Arc<AtomicU32>,
        buffers: mumble_protocol::audio::mixer::SpeakerBuffers,
    ) -> Result<Self> {
        Ok(Self {
            stream: None,
            volume,
            buffers,
        })
    }
}

impl super::MixingPlayback for OboeMixingPlayback {
    fn start(&mut self) -> Result<()> {
        let callback = MixingPlaybackCallback {
            buffers: self.buffers.clone(),
            volume: self.volume.clone(),
            last_sample: 0.0,
        };

        let mut stream = AudioStreamBuilder::default()
            .set_performance_mode(PerformanceMode::LowLatency)
            .set_sharing_mode(SharingMode::Shared)
            .set_format::<f32>()
            .set_channel_count::<Mono>()
            .set_sample_rate(48_000)
            .set_usage(Usage::VoiceCommunication)
            .set_content_type(ContentType::Speech)
            .set_callback(callback)
            .open_stream()
            .map_err(|e| Error::InvalidState(format!("oboe mixing output open: {e:?}")))?;

        stream
            .start()
            .map_err(|e| Error::InvalidState(format!("oboe mixing output start: {e:?}")))?;

        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(mut stream) = self.stream.take() {
            let _ = stream.stop();
        }
        if let Ok(mut bufs) = self.buffers.lock() {
            bufs.clear();
        }
        Ok(())
    }
}
