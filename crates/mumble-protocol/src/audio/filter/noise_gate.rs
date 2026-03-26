//! Simple noise gate that silences audio below a threshold.
//!
//! In the outbound filter chain the noise gate should run **after**
//! the AGC so that it evaluates the post-gain signal level. Without
//! this ordering, platforms whose audio stack pre-processes the mic
//! signal (e.g. Android `VoiceCommunication`) may produce levels too
//! quiet for the gate threshold, causing nearly all speech to be
//! gated.

use std::f32::consts::PI;

use crate::audio::sample::AudioFrame;
use crate::audio::filter::AudioFilter;
use crate::error::Result;

/// Gate states for hysteresis behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateState {
    /// Gate is closed - output silence.
    Closed,
    /// Gate is open - pass audio through.
    Open,
}

/// Configuration for the noise gate.
#[derive(Debug, Clone)]
pub struct NoiseGateConfig {
    /// RMS threshold (linear, 0.0-1.0) below which audio is gated.
    pub open_threshold: f32,
    /// Threshold at which the gate closes again (should be <= `open_threshold`).
    pub close_threshold: f32,
    /// Number of frames to hold the gate open after the signal drops
    /// below `close_threshold` (prevents choppy speech).
    pub hold_frames: u32,
    /// Number of samples for the fade-in when the gate opens
    /// (prevents click at speech start).
    pub attack_samples: usize,
    /// Number of samples for the fade-out when the gate closes  
    /// (prevents click at speech end).
    pub release_samples: usize,
}

impl Default for NoiseGateConfig {
    fn default() -> Self {
        Self {
            open_threshold: 0.01,
            close_threshold: 0.008,
            hold_frames: 10,
            attack_samples: 480,  // 10 ms @ 48 kHz
            release_samples: 480,  // 10 ms @ 48 kHz
        }
    }
}

/// A simple noise gate with hysteresis.
#[derive(Debug)]
pub struct NoiseGate {
    config: NoiseGateConfig,
    state: GateState,
    hold_counter: u32,
    enabled: bool,
}

impl NoiseGate {
    /// Create a new noise gate with the given configuration.
    pub fn new(config: NoiseGateConfig) -> Self {
        Self {
            config,
            state: GateState::Closed,
            hold_counter: 0,
            enabled: true,
        }
    }

    /// Compute the RMS of a slice of f32 samples.
    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }
}

impl AudioFilter for NoiseGate {
    fn name(&self) -> &str {
        "NoiseGate"
    }

    fn process(&mut self, frame: &mut AudioFrame) -> Result<()> {
        let samples = frame.as_f32_samples();
        let level = Self::rms(samples);
        let n = samples.len();

        match self.state {
            GateState::Closed => {
                if level >= self.config.open_threshold {
                    self.state = GateState::Open;
                    self.hold_counter = self.config.hold_frames;
                    frame.is_silent = false;
                    // Raised-cosine fade-in to avoid a click.
                    // Using a Hann curve: gain = 0.5 * (1 - cos(PI * i / n))
                    // produces a smooth S-shape with no sharp corners.
                    let attack = self.config.attack_samples.min(n);
                    if attack > 0 {
                        let samples_mut = frame.as_f32_samples_mut();
                        let inv = 1.0 / attack as f32;
                        for (i, sample) in samples_mut.iter_mut().enumerate().take(attack) {
                            let gain = 0.5 * (1.0 - (PI * i as f32 * inv).cos());
                            *sample *= gain;
                        }
                    }
                } else {
                    // silence the frame
                    frame.data.fill(0);
                    frame.is_silent = true;
                }
            }
            GateState::Open => {
                if level < self.config.close_threshold {
                    if self.hold_counter > 0 {
                        self.hold_counter -= 1;
                        // still holding open - pass through
                        frame.is_silent = false;
                    } else {
                        self.state = GateState::Closed;
                        // Raised-cosine fade-out to avoid a click.
                        let release = self.config.release_samples.min(n);
                        if release > 0 {
                            let samples_mut = frame.as_f32_samples_mut();
                            let start = n - release;
                            let inv = 1.0 / release as f32;
                            for i in 0..release {
                                let gain = 0.5 * (1.0 + (PI * i as f32 * inv).cos());
                                samples_mut[start + i] *= gain;
                            }
                        }
                        frame.is_silent = true;
                    }
                } else {
                    self.hold_counter = self.config.hold_frames;
                    frame.is_silent = false;
                }
            }
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.state = GateState::Closed;
        self.hold_counter = 0;
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
    fn silent_frame_is_gated() -> Result<()> {
        let mut gate = NoiseGate::new(NoiseGateConfig::default());
        let mut frame = make_frame(&[0.0; 480]);
        gate.process(&mut frame)?;
        assert!(frame.data.iter().all(|&b| b == 0));
        Ok(())
    }

    #[test]
    fn loud_frame_passes_through() -> Result<()> {
        let config = NoiseGateConfig::default();
        let attack = config.attack_samples;
        // Frame must be at least as large as the attack region.
        let frame_len = attack.max(480);
        let mut gate = NoiseGate::new(config);
        let samples: Vec<f32> = (0..frame_len)
            .map(|i| (i as f32 / frame_len as f32 * 0.5).sin() * 0.5)
            .collect();
        let mut frame = make_frame(&samples);
        gate.process(&mut frame)?;
        let output = frame.as_f32_samples();

        // The fade-in region should be attenuated (sample 0 = 0).
        assert_eq!(output[0], 0.0, "First sample should be faded to zero");

        // After the attack region, audio should pass through unchanged.
        for i in attack..frame_len {
            assert!(
                (output[i] - samples[i]).abs() < 1e-6,
                "Sample {i} should pass through unchanged"
            );
        }

        // The frame should NOT be marked silent.
        assert!(!frame.is_silent);
        Ok(())
    }
}
