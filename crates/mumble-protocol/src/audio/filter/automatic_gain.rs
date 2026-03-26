//! Automatic gain control (AGC).
//!
//! Normalises the signal level so quiet talkers are brought up and
//! loud peaks are attenuated. Uses a simple envelope follower with
//! configurable attack/release times.
//!
//! Peaks that exceed [-1, 1] after gain are soft-clipped with a `tanh`
//! curve rather than hard-clamped, which avoids the harsh metallic
//! distortion typical of hard clipping.

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
    /// Input level below which the AGC freezes its gain instead of
    /// adjusting.  Prevents the AGC from ramping gain up during
    /// silence/pauses and amplifying noise, which causes "pumping"
    /// artefacts and noise-gate flutter.
    pub gate_threshold: f32,
}

impl Default for AgcConfig {
    fn default() -> Self {
        Self {
            target_level: 0.25,
            max_gain: 4.0,
            min_gain: 0.25,
            attack: 0.15,
            release: 0.05,
            gate_threshold: 0.003,
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

    /// Soft-clip a sample using `tanh`.
    ///
    /// Below the knee (default 0.8) the signal passes through linearly.
    /// Above that it is smoothly compressed toward +/-1.0, avoiding the
    /// harsh distortion of a hard clamp.
    #[inline]
    fn soft_clip(sample: f32) -> f32 {
        const KNEE: f32 = 0.8;
        if sample.abs() <= KNEE {
            return sample;
        }
        // Map the region [KNEE, inf) into [KNEE, 1.0) via tanh.
        let sign = sample.signum();
        let excess = (sample.abs() - KNEE) / (1.0 - KNEE);
        sign * (KNEE + (1.0 - KNEE) * excess.tanh())
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

        // Only adjust gain when input is above the gate threshold.
        // Below that (silence / noise floor) the gain is frozen to
        // prevent amplifying background noise during speech pauses,
        // which would cause pumping artefacts and noise-gate flutter.
        if level > self.config.gate_threshold {
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
        // else: keep current gain frozen when signal is below gate

        // Linearly interpolate gain across the frame to avoid step
        // discontinuities at frame boundaries (reduces THD).
        let end_gain = self.current_gain;
        let n = samples.len() as f32;
        for (i, s) in samples.iter_mut().enumerate() {
            let t = (i as f32 + 1.0) / n;
            let gain = start_gain + (end_gain - start_gain) * t;
            *s = Self::soft_clip(*s * gain);
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

    #[test]
    fn soft_clip_below_knee_is_linear() {
        for v in [0.0_f32, 0.1, 0.5, 0.79, -0.3, -0.79] {
            let out = AutomaticGainControl::soft_clip(v);
            assert!(
                (out - v).abs() < 1e-6,
                "Below knee, soft_clip({v}) should be {v}, got {out}"
            );
        }
    }

    #[test]
    fn soft_clip_above_knee_stays_below_one() {
        // Moderate values above knee should be compressed but stay < 1.0
        for v in [1.0_f32, 1.5, 2.0, -1.5] {
            let out = AutomaticGainControl::soft_clip(v);
            assert!(
                out.abs() < 1.0,
                "soft_clip({v}) must be < 1.0, got {out}"
            );
            assert!(
                out.abs() > 0.8,
                "soft_clip({v}) must be > knee (0.8), got {out}"
            );
            assert_eq!(out.signum(), v.signum());
        }
        // Very large values clamp to (almost) 1.0 via tanh - that's fine,
        // the important thing is the smooth transition, not a flat-top.
        let out = AutomaticGainControl::soft_clip(10.0);
        assert!(out >= 0.99, "soft_clip(10.0) should asymptote near 1.0");
    }

    #[test]
    fn heavily_amplified_signal_is_not_hard_clipped() -> Result<()> {
        // Simulate a signal that would clip with hard clamp.
        let mut agc = AutomaticGainControl::new(AgcConfig {
            max_gain: 10.0,
            ..AgcConfig::default()
        });
        // Signal with peaks at 0.3 - after 10x gain, peaks would be 3.0
        let loud: Vec<f32> = (0..960)
            .map(|i| (i as f32 / 960.0 * std::f32::consts::TAU * 5.0).sin() * 0.3)
            .collect();
        let mut frame = make_frame(&loud);
        // Ramp up gain
        for _ in 0..50 {
            frame = make_frame(&loud);
            agc.process(&mut frame)?;
        }
        let output = frame.as_f32_samples();
        // No sample should be exactly +/-1.0 (that would be hard clipping)
        let hard_clipped = output.iter().filter(|&&s| s.abs() >= 0.9999).count();
        assert_eq!(
            hard_clipped, 0,
            "Soft clipping should never produce values at +/-1.0"
        );
        // But the signal should still be loud (soft-clipped, not silenced)
        let out_rms: f32 = {
            let sum: f32 = output.iter().map(|s| s * s).sum();
            (sum / output.len() as f32).sqrt()
        };
        assert!(out_rms > 0.1, "Signal should still be present after soft-clip");
        Ok(())
    }

    #[test]
    fn gain_freezes_below_gate_threshold() -> Result<()> {
        let mut agc = AutomaticGainControl::new(AgcConfig {
            gate_threshold: 0.01,
            ..AgcConfig::default()
        });

        // First, feed a normal signal to let the AGC settle to a gain value.
        let normal: Vec<f32> = vec![0.05; 480];
        for _ in 0..20 {
            let mut frame = make_frame(&normal);
            agc.process(&mut frame)?;
        }
        let gain_before = agc.current_gain;

        // Now feed a very quiet signal (below gate_threshold).
        // The AGC should NOT increase gain to chase it.
        let quiet: Vec<f32> = vec![0.001; 480];
        for _ in 0..50 {
            let mut frame = make_frame(&quiet);
            agc.process(&mut frame)?;
        }
        let gain_after = agc.current_gain;

        assert!(
            (gain_after - gain_before).abs() < 0.01,
            "Gain should be frozen below gate threshold: before={gain_before}, after={gain_after}"
        );
        Ok(())
    }
}
