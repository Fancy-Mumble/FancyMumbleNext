//! Audio filter interface and filter-chain builder.
//!
//! Every filter implements [`AudioFilter`] and processes frames in-place.
//! Filters are composed into a chain via [`FilterChain`], which runs
//! them sequentially. New filters are added by implementing the trait
//! in a separate file - no existing code needs to change.

pub mod automatic_gain;
pub mod denoiser;
pub mod low_pass;
pub mod noise_gate;
pub mod volume;

use crate::audio::sample::AudioFrame;
use crate::error::Result;

/// A single processing stage that transforms audio in-place.
///
/// Implementations should be lightweight and real-time safe
/// (no allocations, no blocking I/O during [`process`]).
pub trait AudioFilter: Send + 'static {
    /// Human-readable name for logging / UI display.
    fn name(&self) -> &str;

    /// Process a frame of audio **in-place**.
    ///
    /// The filter may modify `frame.data` and must leave
    /// `frame.format` and `frame.sequence` unchanged.
    fn process(&mut self, frame: &mut AudioFrame) -> Result<()>;

    /// Reset any internal state (e.g. between voice transmissions).
    fn reset(&mut self);

    /// Whether this filter is currently enabled.
    fn is_enabled(&self) -> bool;

    /// Enable or disable this filter at runtime.
    fn set_enabled(&mut self, enabled: bool);
}

/// An ordered chain of [`AudioFilter`]s executed sequentially.
///
/// Filters run in insertion order. Disabled filters are skipped
/// automatically.
pub struct FilterChain {
    filters: Vec<Box<dyn AudioFilter>>,
}

impl FilterChain {
    /// Create an empty filter chain.
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// Append a filter to the end of the chain.
    pub fn push(&mut self, filter: Box<dyn AudioFilter>) {
        self.filters.push(filter);
    }

    /// Process a frame through every enabled filter in order.
    pub fn process(&mut self, frame: &mut AudioFrame) -> Result<()> {
        for filter in &mut self.filters {
            if filter.is_enabled() {
                filter.process(frame)?;
            }
        }
        Ok(())
    }

    /// Reset all filters in the chain.
    pub fn reset(&mut self) {
        for filter in &mut self.filters {
            filter.reset();
        }
    }

    /// Number of filters (enabled or not) in the chain.
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    /// Whether the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }
}

impl Default for FilterChain {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FilterChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilterChain")
            .field("filter_count", &self.filters.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::filter::automatic_gain::{AgcConfig, AutomaticGainControl};
    use crate::audio::filter::noise_gate::{NoiseGate, NoiseGateConfig};
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

    /// Regression: a quiet mic signal (e.g. Android `VoiceCommunication`)
    /// must NOT be gated when AGC runs before the noise gate.
    /// With the old order (gate first) the gate would close because the
    /// raw RMS is below the 0.01 threshold.
    #[test]
    fn agc_before_gate_passes_quiet_speech() -> Result<()> {
        // Simulate a quiet mic signal (~0.005 RMS, well below the
        // default gate threshold of 0.01).
        let n = 960; // 20 ms @ 48 kHz
        let quiet_level = 0.005_f32;
        let samples: Vec<f32> = (0..n)
            .map(|i| (i as f32 / n as f32 * std::f32::consts::TAU * 5.0).sin() * quiet_level)
            .collect();

        // Correct order: AGC -> NoiseGate.
        let mut chain = FilterChain::new();
        chain.push(Box::new(AutomaticGainControl::new(AgcConfig {
            max_gain: 10.0_f32.powf(15.0 / 20.0), // 15 dB
            ..AgcConfig::default()
        })));
        chain.push(Box::new(NoiseGate::new(NoiseGateConfig::default())));

        // Feed several frames so the AGC can ramp up its gain.
        let mut frame = make_frame(&samples);
        for _ in 0..30 {
            frame = make_frame(&samples);
            chain.process(&mut frame)?;
        }

        // After ramp-up the frame must NOT be silent.
        assert!(
            !frame.is_silent,
            "Quiet speech should pass when AGC runs before the noise gate"
        );
        Ok(())
    }

    /// Verify that the wrong order (gate first) DOES gate quiet speech,
    /// confirming the regression the ordering fix prevents.
    #[test]
    fn gate_before_agc_gates_quiet_speech() -> Result<()> {
        let n = 960;
        let quiet_level = 0.005_f32;
        let samples: Vec<f32> = (0..n)
            .map(|i| (i as f32 / n as f32 * std::f32::consts::TAU * 5.0).sin() * quiet_level)
            .collect();

        // Wrong order: NoiseGate -> AGC.
        let mut chain = FilterChain::new();
        chain.push(Box::new(NoiseGate::new(NoiseGateConfig::default())));
        chain.push(Box::new(AutomaticGainControl::new(AgcConfig {
            max_gain: 10.0_f32.powf(15.0 / 20.0),
            ..AgcConfig::default()
        })));

        let mut frame = make_frame(&samples);
        for _ in 0..30 {
            frame = make_frame(&samples);
            chain.process(&mut frame)?;
        }

        // With the wrong order, the gate sees raw 0.005 < 0.01 and gates it.
        assert!(
            frame.is_silent,
            "Quiet speech should be gated when noise gate runs before AGC"
        );
        Ok(())
    }
}
