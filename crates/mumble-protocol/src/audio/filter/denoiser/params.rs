//! Dynamic per-algorithm parameter schema.
//!
//! Each [`super::NoiseSuppressionAlgorithm`] exposes a static list of
//! tunable knobs through [`algorithm_param_specs`].  The current
//! values for those knobs are carried in [`DenoiserParams`] (a small,
//! sparse map from parameter id to `f32`).  Backends read the knobs
//! they understand at construction time and ignore the rest, which
//! keeps the wire format and the UI dropdown decoupled from the
//! exact algorithm implementation.
//!
//! Keeping the schema *static* (no per-instance allocation) means the
//! UI can be built ahead of time from a `Vec<DenoiserParamSpec>`
//! returned by the Tauri command layer, with no risk of the spec
//! drifting away from what the backend actually reads.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::NoiseSuppressionAlgorithm;

/// One tunable knob exposed by a denoiser backend.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DenoiserParamSpec {
    /// Stable identifier used by both the UI and the backend.
    pub id: &'static str,
    /// Human-readable label for the UI slider.
    pub label: &'static str,
    /// One-line description shown as a tooltip / help text.
    pub description: &'static str,
    /// Inclusive minimum value for the slider.
    pub min: f32,
    /// Inclusive maximum value for the slider.
    pub max: f32,
    /// UI slider step size.
    pub step: f32,
    /// Default value used when the user has not set this knob.
    pub default: f32,
    /// Optional unit suffix shown next to the value (e.g. `"dB"`).
    pub unit: &'static str,
}

/// Sparse map of `param-id -> value` carried in [`super::DenoiserConfig`].
///
/// Missing entries fall back to [`DenoiserParamSpec::default`].
pub type DenoiserParams = BTreeMap<String, f32>;

/// Read a single knob, applying clamping and the spec's default when
/// the value is missing or outside the allowed range.
#[must_use]
pub(super) fn read_param(params: &DenoiserParams, spec: &DenoiserParamSpec) -> f32 {
    params
        .get(spec.id)
        .copied()
        .map_or(spec.default, |v| v.clamp(spec.min, spec.max))
}

const DEEPFILTER_PARAMS: &[DenoiserParamSpec] = &[
    DenoiserParamSpec {
        id: "atten_lim_db",
        label: "Attenuation limit",
        description: "Hard upper bound on noise attenuation. Lower values keep more residual ambience and avoid the over-suppressed, clipped sound DeepFilterNet can produce at full strength.",
        min: 6.0,
        max: 60.0,
        step: 1.0,
        default: 24.0,
        unit: "dB",
    },
    DenoiserParamSpec {
        id: "post_filter_beta",
        label: "Post-filter strength",
        description: "DeepFilterNet post-filter beta. Set to 0 to disable; values around 0.02 produce cleaner output without audibly over-smoothing speech.",
        min: 0.0,
        max: 0.05,
        step: 0.005,
        default: 0.02,
        unit: "",
    },
    DenoiserParamSpec {
        id: "min_db_thresh",
        label: "Voice-activity floor",
        description: "Frames whose model-estimated SNR falls below this dB level are passed through untouched, which avoids 'pumping' on near-silent input.",
        min: -20.0,
        max: 0.0,
        step: 1.0,
        default: -10.0,
        unit: "dB",
    },
];

const OMLSA_PARAMS: &[DenoiserParamSpec] = &[DenoiserParamSpec {
    id: "min_gain",
    label: "Gain floor",
    description: "Minimum per-bin gain. Lower values suppress more noise but can introduce 'breathing' artefacts; higher values keep more residual ambience.",
    min: 0.01,
    max: 0.5,
    step: 0.01,
    default: 0.05,
    unit: "",
}];

const SPECTRAL_PARAMS: &[DenoiserParamSpec] = &[DenoiserParamSpec {
    id: "min_gain",
    label: "Gain floor",
    description: "Minimum per-bin gain. Lower values suppress more noise but can introduce musical-noise artefacts; higher values sound more natural.",
    min: 0.01,
    max: 0.5,
    step: 0.01,
    default: 0.05,
    unit: "",
}];

/// Specs for every knob the given algorithm exposes.
///
/// Returns an empty slice for algorithms that do not expose any
/// tunable parameters (e.g. [`NoiseSuppressionAlgorithm::None`] or
/// [`NoiseSuppressionAlgorithm::Rnnoise`]).
#[must_use]
pub fn algorithm_param_specs(algorithm: NoiseSuppressionAlgorithm) -> &'static [DenoiserParamSpec] {
    match algorithm {
        NoiseSuppressionAlgorithm::None | NoiseSuppressionAlgorithm::Rnnoise => &[],
        NoiseSuppressionAlgorithm::DeepFilterNet => DEEPFILTER_PARAMS,
        NoiseSuppressionAlgorithm::OmlsaImcra => OMLSA_PARAMS,
        NoiseSuppressionAlgorithm::SpectralSubtraction => SPECTRAL_PARAMS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_algorithm_has_a_spec_list() {
        for algo in NoiseSuppressionAlgorithm::ALL {
            // Accessing the spec list must never panic and the spec
            // metadata must be self-consistent.
            for spec in algorithm_param_specs(algo) {
                assert!(spec.min < spec.max, "{}: min < max", spec.id);
                assert!(spec.step > 0.0, "{}: step > 0", spec.id);
                assert!(
                    spec.default >= spec.min && spec.default <= spec.max,
                    "{}: default in range",
                    spec.id
                );
                assert!(!spec.id.is_empty());
                assert!(!spec.label.is_empty());
            }
        }
    }

    #[test]
    fn read_param_falls_back_to_default() {
        let spec = &DEEPFILTER_PARAMS[0];
        let empty = DenoiserParams::new();
        assert!((read_param(&empty, spec) - spec.default).abs() < f32::EPSILON);
    }

    #[test]
    fn read_param_clamps_out_of_range_values() {
        let spec = &DEEPFILTER_PARAMS[0];
        let mut params = DenoiserParams::new();
        let _ = params.insert(spec.id.into(), spec.max + 100.0);
        assert!((read_param(&params, spec) - spec.max).abs() < f32::EPSILON);
        let _ = params.insert(spec.id.into(), spec.min - 100.0);
        assert!((read_param(&params, spec) - spec.min).abs() < f32::EPSILON);
    }
}
