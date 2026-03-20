//! Audio playback abstraction (pipeline output stage).
//!
//! Implement [`AudioPlayback`] to send decoded PCM audio to any
//! hardware or virtual output device.

use crate::audio::sample::{AudioFormat, AudioFrame};
use crate::error::Result;

/// Trait for playing audio on an output device.
///
/// Implementations interact with the OS audio subsystem.
/// The pipeline only depends on this trait.
pub trait AudioPlayback: Send + 'static {
    /// The native input format expected by this playback device.
    fn format(&self) -> AudioFormat;

    /// Write a frame of audio to the output device.
    ///
    /// The frame must match (or be convertible to) the format returned
    /// by [`format()`](AudioPlayback::format).
    fn write_frame(&mut self, frame: &AudioFrame) -> Result<()>;

    /// Start the playback device.
    fn start(&mut self) -> Result<()>;

    /// Stop the playback device.
    fn stop(&mut self) -> Result<()>;
}

// -- Null playback (testing / headless) -----------------------------

/// A playback sink that discards all audio. Useful for testing or bots.
#[derive(Debug)]
pub struct NullPlayback {
    format: AudioFormat,
}

impl NullPlayback {
    /// Create a new null playback sink with the given format.
    pub fn new(format: AudioFormat) -> Self {
        Self { format }
    }
}

impl AudioPlayback for NullPlayback {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn write_frame(&mut self, _frame: &AudioFrame) -> Result<()> {
        Ok(())
    }

    fn start(&mut self) -> Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}
