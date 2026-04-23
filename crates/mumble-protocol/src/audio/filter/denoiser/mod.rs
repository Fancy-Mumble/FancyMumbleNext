//! Pluggable noise-suppression backends.
//!
//! Each backend implements [`DenoiserBackend`] and can be selected at
//! runtime via [`NoiseSuppressionAlgorithm`].  Switching algorithms
//! does NOT require a recompile and is wired through the audio
//! settings: the outbound pipeline rebuilds itself when the user picks
//! a different algorithm in the UI.
//!
//! ## Backends shipped today
//!
//! | Algorithm | Type | Strengths | CPU |
//! |-----------|------|-----------|-----|
//! | [`NoiseSuppressionAlgorithm::Rnnoise`] | RNN (GRU) | Excellent on non-stationary noise (typing, fans), well-rounded default | ~5% of one core |
//! | [`NoiseSuppressionAlgorithm::OmlsaImcra`] | Classical DSP (Cohen 2001 OMLSA + Cohen 2003 IMCRA) | Smoother than spectral subtraction, fewer musical-noise artefacts | ~2% of one core |
//! | [`NoiseSuppressionAlgorithm::SpectralSubtraction`] | Classical DSP (Martin 2001 minimum-statistics PSD tracker + Wiener gain) | Very low CPU, no model file, predictable artefacts | ~1% of one core |
//! | [`NoiseSuppressionAlgorithm::None`] | Pass-through | Reference / debugging | 0 |
//!
//! ## Deep-learning SOTA
//!
//! [`NoiseSuppressionAlgorithm::DeepFilterNet`] wires in Schroeter et
//! al.'s `DeepFilterNet3` (INTERSPEECH 2023,
//! [arXiv:2305.08227](https://arxiv.org/abs/2305.08227)) through the
//! dedicated [`fancy_denoiser_deepfilter`] crate.  Enabled via the
//! `deepfilternet-denoiser` cargo feature (adds ~5 MiB of embedded
//! ONNX weights + the `tract-onnx` runtime).  Falls back to `RNNoise`
//! when the feature is not compiled in.

use serde::{Deserialize, Serialize};

use crate::audio::filter::AudioFilter;
use crate::audio::sample::{AudioFrame, SampleFormat};
use crate::error::Result;

#[cfg(feature = "deepfilternet-denoiser")]
mod deepfilter;
mod omlsa;
mod params;
mod rnnoise;
mod spectral_subtraction;

#[cfg(feature = "deepfilternet-denoiser")]
use self::deepfilter::DeepFilterBackend;
use self::omlsa::OmlsaBackend;
pub use self::params::{algorithm_param_specs, DenoiserParamSpec, DenoiserParams};
#[cfg(feature = "rnnoise-denoiser")]
use self::rnnoise::RnnoiseBackend;
use self::spectral_subtraction::SpectralSubtractionBackend;

/// Selectable noise-suppression algorithm.
///
/// Stored in `AudioSettings` and serialised with
/// `#[serde(rename_all = "snake_case")]` so the on-the-wire form is
/// `"none"`, `"rnnoise"`, `"spectral_subtraction"`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum NoiseSuppressionAlgorithm {
    /// Disable spectral noise suppression (the noise gate may still run).
    None,
    /// `RNNoise` GRU-based denoiser via the `nnnoiseless` crate.
    /// Falls back to pass-through when the `rnnoise-denoiser` cargo
    /// feature is not enabled.
    #[default]
    Rnnoise,
    /// Classical DSP denoiser based on Martin (2001) minimum-statistics
    /// PSD estimation with a Wiener-style gain rule.  Works without
    /// any extra dependencies and is the recommended fallback for
    /// constrained devices.
    SpectralSubtraction,
    /// OMLSA + IMCRA (Cohen 2001/2003) - the canonical "modern
    /// classical" speech enhancer.  Inlined inside this crate, always
    /// available.  Markedly less prone to musical-noise artefacts than
    /// `SpectralSubtraction` at roughly 2x the CPU cost.
    OmlsaImcra,
    /// `DeepFilterNet3` - deep-learning SOTA (Schroeter et al. 2023).
    /// Provided by the [`fancy_denoiser_deepfilter`] crate.  Falls back
    /// to `Rnnoise` when the `deepfilternet-denoiser` cargo feature is
    /// not enabled.
    DeepFilterNet,
}

impl NoiseSuppressionAlgorithm {
    /// All variants in display order (used by the UI dropdown).
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::Rnnoise,
        Self::DeepFilterNet,
        Self::OmlsaImcra,
        Self::SpectralSubtraction,
    ];

    /// Human-readable label for UI display.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Off",
            Self::Rnnoise => "RNNoise (recurrent neural network)",
            Self::DeepFilterNet => "DeepFilterNet (deep-learning SOTA)",
            Self::OmlsaImcra => "OMLSA + IMCRA (modern classical)",
            Self::SpectralSubtraction => "Spectral subtraction (low-CPU classical)",
        }
    }
}

/// Configuration for the denoiser.
#[derive(Debug, Clone)]
pub struct DenoiserConfig {
    /// Algorithm to use.  Default is [`NoiseSuppressionAlgorithm::Rnnoise`].
    pub algorithm: NoiseSuppressionAlgorithm,
    /// Strength of noise attenuation, `0.0` (disabled) to `1.0` (full).
    ///
    /// Internally this is the wet/dry mix:
    /// `out = (1 - a) * input + a * denoised`.  Values around
    /// `0.85`-`1.0` are typical.  Lower values keep some natural
    /// ambience.
    pub attenuation: f32,
    /// Per-algorithm tunable parameters keyed by
    /// [`DenoiserParamSpec::id`].  Missing entries fall back to the
    /// spec's default; see [`algorithm_param_specs`] for the
    /// available knobs.
    pub params: DenoiserParams,
}

impl Default for DenoiserConfig {
    fn default() -> Self {
        Self {
            algorithm: NoiseSuppressionAlgorithm::default(),
            attenuation: 1.0,
            params: DenoiserParams::new(),
        }
    }
}

/// Internal trait every backend implements.
trait DenoiserBackend: Send {
    fn process(&mut self, samples: &mut [f32], attenuation: f32);
    fn reset(&mut self);
}

/// A pluggable noise suppressor.
///
/// The active backend is chosen by `config.algorithm` and can be
/// swapped via [`SpectralDenoiser::set_algorithm`] without recreating
/// the surrounding pipeline (use [`AudioFilter::reset`] afterwards if
/// you want to flush internal state, but `set_algorithm` already does
/// so implicitly).
pub struct SpectralDenoiser {
    config: DenoiserConfig,
    enabled: bool,
    backend: Box<dyn DenoiserBackend>,
}

impl std::fmt::Debug for SpectralDenoiser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpectralDenoiser")
            .field("config", &self.config)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl SpectralDenoiser {
    /// Create a new denoiser with the given configuration.
    pub fn new(config: DenoiserConfig) -> Self {
        let backend = make_backend(config.algorithm, &config.params);
        Self {
            config,
            enabled: true,
            backend,
        }
    }

    /// Currently selected algorithm.
    pub fn algorithm(&self) -> NoiseSuppressionAlgorithm {
        self.config.algorithm
    }

    /// Switch backends at runtime.  Internal buffers are flushed.
    pub fn set_algorithm(&mut self, algorithm: NoiseSuppressionAlgorithm) {
        if algorithm == self.config.algorithm {
            return;
        }
        self.config.algorithm = algorithm;
        self.backend = make_backend(algorithm, &self.config.params);
    }
}

impl AudioFilter for SpectralDenoiser {
    fn name(&self) -> &str {
        "SpectralDenoiser"
    }

    fn process(&mut self, frame: &mut AudioFrame) -> Result<()> {
        if frame.format.sample_format != SampleFormat::F32
            || frame.format.channels != 1
            || frame.format.sample_rate != 48_000
        {
            return Ok(());
        }
        let attenuation = self.config.attenuation.clamp(0.0, 1.0);
        self.backend.process(frame.as_f32_samples_mut(), attenuation);
        Ok(())
    }

    fn reset(&mut self) {
        self.backend.reset();
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

fn make_backend(
    algorithm: NoiseSuppressionAlgorithm,
    params: &DenoiserParams,
) -> Box<dyn DenoiserBackend> {
    match algorithm {
        NoiseSuppressionAlgorithm::None => Box::new(PassthroughBackend),
        NoiseSuppressionAlgorithm::Rnnoise => make_rnnoise_backend(params),
        NoiseSuppressionAlgorithm::DeepFilterNet => make_deepfilter_backend(params),
        NoiseSuppressionAlgorithm::OmlsaImcra => Box::new(OmlsaBackend::new(params)),
        NoiseSuppressionAlgorithm::SpectralSubtraction => {
            Box::new(SpectralSubtractionBackend::new(params))
        }
    }
}

#[cfg(feature = "rnnoise-denoiser")]
fn make_rnnoise_backend(_params: &DenoiserParams) -> Box<dyn DenoiserBackend> {
    Box::new(RnnoiseBackend::new())
}

#[cfg(not(feature = "rnnoise-denoiser"))]
fn make_rnnoise_backend(params: &DenoiserParams) -> Box<dyn DenoiserBackend> {
    // Without the cargo feature there is no RNN backend - fall back to
    // OMLSA (always available) so users still get noise suppression.
    Box::new(OmlsaBackend::new(params))
}

#[cfg(feature = "deepfilternet-denoiser")]
fn make_deepfilter_backend(params: &DenoiserParams) -> Box<dyn DenoiserBackend> {
    Box::new(DeepFilterBackend::new(params))
}

#[cfg(not(feature = "deepfilternet-denoiser"))]
fn make_deepfilter_backend(params: &DenoiserParams) -> Box<dyn DenoiserBackend> {
    // Without the cargo feature the DNN backend is not linked in -
    // fall back to RNNoise (also a learned denoiser).
    make_rnnoise_backend(params)
}

struct PassthroughBackend;

impl DenoiserBackend for PassthroughBackend {
    fn process(&mut self, _samples: &mut [f32], _attenuation: f32) {}
    fn reset(&mut self) {}
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code: panicking on serde failure is intended")]
mod tests {
    use super::*;
    use crate::audio::sample::AudioFormat;

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

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

    fn pseudo_noise(seed: &mut u32, n: usize, level: f32) -> Vec<f32> {
        (0..n)
            .map(|_| {
                *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (((*seed >> 16) & 0x7FFF) as f32 / 32_768.0 - 0.5) * level
            })
            .collect()
    }

    #[test]
    fn algorithm_label_and_serde_roundtrip() {
        for algo in NoiseSuppressionAlgorithm::ALL {
            assert!(!algo.label().is_empty());
            let json = serde_json::to_string(&algo).unwrap();
            let back: NoiseSuppressionAlgorithm = serde_json::from_str(&json).unwrap();
            assert_eq!(algo, back, "round-trip failed for {algo:?}");
        }
    }

    #[test]
    fn algorithm_serde_uses_snake_case() {
        let json = serde_json::to_string(&NoiseSuppressionAlgorithm::SpectralSubtraction).unwrap();
        assert_eq!(json, "\"spectral_subtraction\"");
    }

    #[test]
    fn none_algorithm_is_passthrough() -> Result<()> {
        let mut denoiser = SpectralDenoiser::new(DenoiserConfig {
            algorithm: NoiseSuppressionAlgorithm::None,
            attenuation: 1.0,
            ..Default::default()
        });
        let samples: Vec<f32> = (0..960).map(|i| (i as f32 * 0.001).sin() * 0.1).collect();
        let mut frame = make_frame(&samples);
        denoiser.process(&mut frame)?;
        for (a, b) in frame.as_f32_samples().iter().zip(samples.iter()) {
            assert!((a - b).abs() < 1e-6, "None algorithm must not modify samples");
        }
        Ok(())
    }

    /// Regression: switching algorithms at runtime must not crash and
    /// must produce a working denoiser of the requested type.
    #[test]
    fn set_algorithm_switches_backend() -> Result<()> {
        let mut denoiser = SpectralDenoiser::new(DenoiserConfig {
            algorithm: NoiseSuppressionAlgorithm::None,
            attenuation: 1.0,
            ..Default::default()
        });
        assert_eq!(denoiser.algorithm(), NoiseSuppressionAlgorithm::None);
        denoiser.set_algorithm(NoiseSuppressionAlgorithm::SpectralSubtraction);
        assert_eq!(
            denoiser.algorithm(),
            NoiseSuppressionAlgorithm::SpectralSubtraction
        );

        let mut seed = 0xDEAD_BEEF_u32;
        let samples = pseudo_noise(&mut seed, 960, 0.1);
        let mut frame = make_frame(&samples);
        let in_rms = rms(frame.as_f32_samples());
        for _ in 0..40 {
            let s = pseudo_noise(&mut seed, 960, 0.1);
            frame = make_frame(&s);
            denoiser.process(&mut frame)?;
        }
        let out_rms = rms(frame.as_f32_samples());
        assert!(
            out_rms < in_rms,
            "spectral subtraction should attenuate noise after switching backends; in={in_rms:.5}, out={out_rms:.5}"
        );
        Ok(())
    }

    /// Format guard: non-supported formats must pass through untouched
    /// for every algorithm.
    #[test]
    fn unsupported_format_passthrough_for_all_algorithms() -> Result<()> {
        for algo in NoiseSuppressionAlgorithm::ALL {
            let mut denoiser = SpectralDenoiser::new(DenoiserConfig {
                algorithm: algo,
                attenuation: 1.0,
                ..Default::default()
            });
            let samples = vec![0.5_f32; 960];
            let mut frame = make_frame(&samples);
            frame.format.sample_rate = 16_000; // unsupported
            denoiser.process(&mut frame)?;
            assert!(
                frame
                    .as_f32_samples()
                    .iter()
                    .all(|&s| (s - 0.5).abs() < 1e-6),
                "{algo:?} must pass through unsupported format unchanged"
            );
        }
        Ok(())
    }

    /// Disable toggle is honoured by every algorithm.
    #[test]
    fn disable_toggle() {
        for algo in NoiseSuppressionAlgorithm::ALL {
            let mut d = SpectralDenoiser::new(DenoiserConfig {
                algorithm: algo,
                attenuation: 1.0,
                ..Default::default()
            });
            assert!(d.is_enabled());
            d.set_enabled(false);
            assert!(!d.is_enabled());
        }
    }
}
