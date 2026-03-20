//! AI-style spectral denoiser stub.
//!
//! In production this would wrap an inference engine running a trained
//! model (e.g. `RNNoise`, `DeepFilterNet`, or a custom ONNX model).
//! The trait boundary is already set so that a real back-end can be
//! swapped in without changing any pipeline code.
//!
//! The default [`SpectralDenoiser`] is a no-op placeholder that simply
//! passes audio through.

use crate::audio::sample::AudioFrame;
use crate::audio::filter::AudioFilter;
use crate::error::Result;

/// Configuration for AI-based denoiser.
#[derive(Debug, Clone)]
pub struct DenoiserConfig {
    /// Strength of noise attenuation (0.0 = off, 1.0 = maximum).
    pub attenuation: f32,
}

impl Default for DenoiserConfig {
    fn default() -> Self {
        Self { attenuation: 1.0 }
    }
}

/// Placeholder for a real ML-based denoiser (`RNNoise` / `DeepFilterNet`
/// / custom ONNX). Currently a passthrough - plug a real implementation
/// behind `AudioFilter` when the inference back-end is available.
#[derive(Debug)]
pub struct SpectralDenoiser {
    #[allow(dead_code, reason = "reserved for future ML-based denoiser implementation")]
    config: DenoiserConfig,
    enabled: bool,
}

impl SpectralDenoiser {
    /// Create a new spectral denoiser with the given configuration.
    pub fn new(config: DenoiserConfig) -> Self {
        Self {
            config,
            enabled: true,
        }
    }
}

impl AudioFilter for SpectralDenoiser {
    fn name(&self) -> &str {
        "SpectralDenoiser"
    }

    fn process(&mut self, _frame: &mut AudioFrame) -> Result<()> {
        // TODO: replace with real inference call.
        // A production implementation would:
        //   1. Convert frame to f32 mono @ model sample rate
        //   2. Feed overlapping windows into the model
        //   3. Multiply by the predicted mask in the frequency domain
        //   4. Overlap-add back to time domain
        //   5. Write result into frame.data
        Ok(())
    }

    fn reset(&mut self) {
        // Would reset internal ring buffers / model state.
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}
