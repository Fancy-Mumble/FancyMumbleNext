//! Automatic gain control (AGC).
//!
//! Normalises the signal level so quiet talkers are brought up and
//! loud peaks are attenuated. Uses a simple envelope follower with
//! configurable attack/release times.

use crate::audio::sample::AudioFrame;
use crate::audio::filter::AudioFilter;
use crate::error::Result;

/// Configuration for the AGC.
#[derive(Debug, Clone)]
pub struct AgcConfig {
    /// Target RMS level (linear, 0.0-1.0).
    pub target_level: f32,
    /// Maximum gain that can be applied (prevents amplifying noise).
    pub max_gain: f32,
    /// Minimum gain (prevents total silence on loud signals).
    pub min_gain: f32,
    /// Attack coefficient per frame (0.0-1.0). Smaller = slower reaction
    /// to increasing volume.
    pub attack: f32,
    /// Release coefficient per frame (0.0-1.0). Smaller = slower reaction
    /// to decreasing volume.
    pub release: f32,
}

impl Default for AgcConfig {
    fn default() -> Self {
        Self {
            target_level: 0.25,
            max_gain: 4.0,
            min_gain: 0.25,
            attack: 0.15,
            release: 0.05,
        }
    }
}

/// A simple envelope-follower AGC.
#[derive(Debug)]
pub struct AutomaticGainControl {
    config: AgcConfig,
    current_gain: f32,
    enabled: bool,
}

impl AutomaticGainControl {
    /// Create a new AGC with the given configuration.
    pub fn new(config: AgcConfig) -> Self {
        Self {
            config,
            current_gain: 1.0,
            enabled: true,
        }
    }

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }
}

impl AudioFilter for AutomaticGainControl {
    fn name(&self) -> &str {
        "AGC"
    }

    fn process(&mut self, frame: &mut AudioFrame) -> Result<()> {
        let samples = frame.as_f32_samples_mut();
        let level = Self::rms(samples);

        let start_gain = self.current_gain;

        if level > 1e-6 {
            let desired_gain = (self.config.target_level / level)
                .clamp(self.config.min_gain, self.config.max_gain);

            // smooth gain changes
            let coeff = if desired_gain < self.current_gain {
                self.config.attack
            } else {
                self.config.release
            };
            self.current_gain += coeff * (desired_gain - self.current_gain);
        }
        // else: keep current gain when signal is negligible

        // Linearly interpolate gain across the frame to avoid step
        // discontinuities at frame boundaries (reduces THD).
        let end_gain = self.current_gain;
        let n = samples.len() as f32;
        for (i, s) in samples.iter_mut().enumerate() {
            let t = (i as f32 + 1.0) / n;
            let gain = start_gain + (end_gain - start_gain) * t;
            *s = (*s * gain).clamp(-1.0, 1.0);
        }

        Ok(())
    }

    fn reset(&mut self) {
        self.current_gain = 1.0;
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
    fn quiet_signal_is_amplified() -> Result<()> {
        let mut agc = AutomaticGainControl::new(AgcConfig::default());
        // Very quiet signal
        let quiet: Vec<f32> = vec![0.001; 480];
        let mut frame = make_frame(&quiet);
        // Process several frames to let gain ramp up
        for _ in 0..50 {
            frame = make_frame(&quiet);
            agc.process(&mut frame)?;
        }
        let output = frame.as_f32_samples();
        let out_rms: f32 = {
            let sum: f32 = output.iter().map(|s| s * s).sum();
            (sum / output.len() as f32).sqrt()
        };
        // Output should be louder than input
        assert!(out_rms > 0.001, "AGC should amplify quiet signal");
        Ok(())
    }
}
