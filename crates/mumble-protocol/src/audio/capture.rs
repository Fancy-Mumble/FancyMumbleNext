//! Audio capture abstraction (pipeline input stage).
//!
//! Implement [`AudioCapture`] to feed PCM audio from any hardware or
//! virtual source into the outbound pipeline.

use crate::audio::sample::{AudioFormat, AudioFrame};
use crate::error::Result;

/// Trait for capturing audio from an input device.
///
/// Implementations are responsible for interacting with the OS audio
/// subsystem (WASAPI, `PulseAudio`, ALSA, `CoreAudio`, …). The pipeline
/// only ever sees this trait, keeping platform code isolated.
pub trait AudioCapture: Send + 'static {
    /// The native output format of this capture device.
    fn format(&self) -> AudioFormat;

    /// Read the next frame of audio.
    ///
    /// Blocks (or awaits internally) until a full frame is available.
    /// Returns `Err` on device errors or when the device is closed.
    fn read_frame(&mut self) -> Result<AudioFrame>;

    /// Start the capture device.
    fn start(&mut self) -> Result<()>;

    /// Stop the capture device.
    fn stop(&mut self) -> Result<()>;
}

// ── Null capture (testing / headless) ──────────────────────────────

/// A silent capture source that produces empty frames at a fixed interval.
///
/// Useful for tests, bots, or headless operation.
pub struct SilentCapture {
    format: AudioFormat,
    frame_size: usize,
    sequence: u64,
}

impl SilentCapture {
    pub fn new(format: AudioFormat, frame_size_samples: usize) -> Self {
        Self {
            format,
            frame_size: frame_size_samples,
            sequence: 0,
        }
    }
}

impl AudioCapture for SilentCapture {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn read_frame(&mut self) -> Result<AudioFrame> {
        let bytes_per_sample = match self.format.sample_format {
            crate::audio::sample::SampleFormat::I16 => 2,
            crate::audio::sample::SampleFormat::F32 => 4,
        };
        let total_bytes = self.frame_size * self.format.channels as usize * bytes_per_sample;
        let frame = AudioFrame {
            data: vec![0u8; total_bytes],
            format: self.format,
            sequence: self.sequence,
            is_silent: false,
        };
        self.sequence += 1;
        Ok(frame)
    }

    fn start(&mut self) -> Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}
