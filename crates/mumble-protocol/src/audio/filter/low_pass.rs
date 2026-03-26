//! Second-order Butterworth low-pass filter (biquad).
//!
//! Removes high-frequency noise/hiss while preserving speech.
//! Designed primarily for Android where the phone microphone
//! often introduces high-frequency electronic noise that the
//! platform's built-in noise suppression does not fully remove.
//!
//! The filter runs at 12 dB/octave roll-off, which is steep enough
//! to meaningfully attenuate hiss above the cutoff while introducing
//! minimal phase distortion in the speech band.

use std::f32::consts::PI;

use crate::audio::filter::AudioFilter;
use crate::audio::sample::AudioFrame;
use crate::error::Result;

/// Configuration for the biquad low-pass filter.
#[derive(Debug, Clone)]
pub struct LowPassConfig {
    /// Cutoff frequency in Hz.
    pub cutoff_hz: f32,
    /// Sample rate in Hz.
    pub sample_rate: f32,
    /// Quality factor (0.7071 = Butterworth, maximally flat passband).
    pub q: f32,
}

impl Default for LowPassConfig {
    fn default() -> Self {
        Self {
            cutoff_hz: 8000.0,
            sample_rate: 48000.0,
            q: std::f32::consts::FRAC_1_SQRT_2, // 0.7071 = Butterworth
        }
    }
}

/// Biquad coefficients (normalised by a0).
#[derive(Debug, Clone)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// Second-order IIR low-pass filter implementing [`AudioFilter`].
#[derive(Debug)]
pub struct LowPassFilter {
    coeffs: BiquadCoeffs,
    // Filter state (two previous samples).
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    enabled: bool,
}

impl LowPassFilter {
    /// Create a new low-pass filter from the given configuration.
    pub fn new(config: &LowPassConfig) -> Self {
        let coeffs = Self::compute_coefficients(config);
        Self {
            coeffs,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            enabled: true,
        }
    }

    /// Compute biquad coefficients for a second-order Butterworth LPF.
    fn compute_coefficients(config: &LowPassConfig) -> BiquadCoeffs {
        let omega = 2.0 * PI * config.cutoff_hz / config.sample_rate;
        let sin_w = omega.sin();
        let cos_w = omega.cos();
        let alpha = sin_w / (2.0 * config.q);

        let a0 = 1.0 + alpha;
        let inv_a0 = 1.0 / a0;

        BiquadCoeffs {
            b0: ((1.0 - cos_w) / 2.0) * inv_a0,
            b1: (1.0 - cos_w) * inv_a0,
            b2: ((1.0 - cos_w) / 2.0) * inv_a0,
            a1: (-2.0 * cos_w) * inv_a0,
            a2: (1.0 - alpha) * inv_a0,
        }
    }

    /// Process a single sample through the biquad.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.coeffs.b0 * x
            + self.coeffs.b1 * self.x1
            + self.coeffs.b2 * self.x2
            - self.coeffs.a1 * self.y1
            - self.coeffs.a2 * self.y2;

        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;

        y
    }
}

impl AudioFilter for LowPassFilter {
    fn name(&self) -> &str {
        "LowPass"
    }

    fn process(&mut self, frame: &mut AudioFrame) -> Result<()> {
        let samples = frame.as_f32_samples_mut();
        for s in samples.iter_mut() {
            *s = self.tick(*s);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
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

    /// Generate a sine wave at the given frequency and sample rate.
    fn sine_wave(freq_hz: f32, sample_rate: f32, num_samples: usize) -> Vec<f32> {
        (0..num_samples)
            .map(|i| (2.0 * PI * freq_hz * i as f32 / sample_rate).sin())
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

    #[test]
    fn low_frequency_passes_through() -> Result<()> {
        let config = LowPassConfig::default(); // 8 kHz cutoff
        let mut filter = LowPassFilter::new(&config);

        // 1 kHz tone - well below cutoff, should pass with minimal loss
        let tone = sine_wave(1000.0, 48000.0, 4800); // 100ms
        let input_rms = rms(&tone);

        let mut frame = make_frame(&tone);
        // Warm up (biquad needs a few samples to settle)
        filter.process(&mut frame)?;
        let mut frame = make_frame(&sine_wave(1000.0, 48000.0, 4800));
        filter.process(&mut frame)?;

        let output_rms = rms(frame.as_f32_samples());
        let attenuation_db = 20.0 * (output_rms / input_rms).log10();

        // Should lose less than 1 dB at 1 kHz
        assert!(
            attenuation_db > -1.0,
            "1 kHz should pass through with < 1 dB loss, got {attenuation_db:.2} dB"
        );
        Ok(())
    }

    #[test]
    fn high_frequency_is_attenuated() -> Result<()> {
        let config = LowPassConfig::default(); // 8 kHz cutoff
        let mut filter = LowPassFilter::new(&config);

        // Warm up with a few frames
        for _ in 0..5 {
            let mut frame = make_frame(&sine_wave(14000.0, 48000.0, 960));
            filter.process(&mut frame)?;
        }

        // 14 kHz tone - well above cutoff, should be strongly attenuated
        let tone = sine_wave(14000.0, 48000.0, 4800);
        let input_rms = rms(&tone);

        let mut frame = make_frame(&tone);
        filter.process(&mut frame)?;

        let output_rms = rms(frame.as_f32_samples());
        let attenuation_db = 20.0 * (output_rms / input_rms).log10();

        // At 14 kHz (0.8 octaves above 8 kHz cutoff), expect > 6 dB attenuation
        assert!(
            attenuation_db < -6.0,
            "14 kHz should be attenuated > 6 dB, got {attenuation_db:.2} dB"
        );
        Ok(())
    }

    #[test]
    fn reset_clears_state() -> Result<()> {
        let config = LowPassConfig::default();
        let mut filter = LowPassFilter::new(&config);

        // Process some audio
        let mut frame = make_frame(&sine_wave(1000.0, 48000.0, 960));
        filter.process(&mut frame)?;

        // Reset
        filter.reset();

        assert_eq!(filter.x1, 0.0);
        assert_eq!(filter.x2, 0.0);
        assert_eq!(filter.y1, 0.0);
        assert_eq!(filter.y2, 0.0);
        Ok(())
    }
}
