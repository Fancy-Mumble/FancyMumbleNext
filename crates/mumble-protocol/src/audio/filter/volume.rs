//! Simple software volume control.
//!
//! Applies a linear gain factor to every sample. Useful both for user
//! volume knobs and as a lightweight mixer building-block.

use crate::audio::sample::AudioFrame;
use crate::audio::filter::AudioFilter;
use crate::error::Result;

/// Configurable software volume (linear gain).
#[derive(Debug)]
pub struct VolumeFilter {
    /// Linear gain factor (1.0 = unity, 0.0 = mute).
    gain: f32,
    enabled: bool,
}

impl VolumeFilter {
    /// Create a new volume filter with the given linear gain.
    pub fn new(gain: f32) -> Self {
        Self {
            gain,
            enabled: true,
        }
    }

    /// Set the gain. Values are clamped to `0.0..=10.0`.
    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain.clamp(0.0, 10.0);
    }

    /// Current gain value.
    pub fn gain(&self) -> f32 {
        self.gain
    }
}

impl AudioFilter for VolumeFilter {
    fn name(&self) -> &str {
        "Volume"
    }

    fn process(&mut self, frame: &mut AudioFrame) -> Result<()> {
        let samples = frame.as_f32_samples_mut();
        for s in samples.iter_mut() {
            *s = (*s * self.gain).clamp(-1.0, 1.0);
        }
        Ok(())
    }

    fn reset(&mut self) {
        // Nothing to reset - volume is stateless.
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::sample::{AudioFormat, SampleFormat};

    fn make_frame(samples: &[f32]) -> AudioFrame {
        let mut data = Vec::with_capacity(samples.len() * 4);
        for &s in samples {
            data.extend_from_slice(&s.to_le_bytes());
        }
        AudioFrame {
            data,
            format: AudioFormat {
                sample_rate: 48_000,
                channels: 1,
                sample_format: SampleFormat::F32,
            },
            sequence: 0,
            is_silent: false,
        }
    }

    #[test]
    fn halving_volume() -> Result<()> {
        let mut vol = VolumeFilter::new(0.5);
        let mut frame = make_frame(&[0.4, -0.6, 0.8]);
        vol.process(&mut frame)?;
        let out = frame.as_f32_samples();
        assert!((out[0] - 0.2).abs() < 1e-6);
        assert!((out[1] - (-0.3)).abs() < 1e-6);
        assert!((out[2] - 0.4).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn mute() -> Result<()> {
        let mut vol = VolumeFilter::new(0.0);
        let mut frame = make_frame(&[0.5, -0.5]);
        vol.process(&mut frame)?;
        let out = frame.as_f32_samples();
        assert!(out.iter().all(|&s| s == 0.0));
        Ok(())
    }
}
