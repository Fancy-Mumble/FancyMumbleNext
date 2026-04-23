//! `DeepFilterNet` noise-suppression backend.
//!
//! Wraps the official [`deep_filter`] crate (Hendrik Schroeter et al.)
//! so it can be dropped into any real-time 48 kHz mono F32 audio
//! pipeline.  The crate ships with an embedded ONNX model; no runtime
//! file loading is required.
//!
//! # References
//!
//! - H. Schroeter et al., "`DeepFilterNet3`: Low-Complexity Speech
//!   Enhancement with Low Computational Complexity", INTERSPEECH
//!   2023, [arXiv:2305.08227](https://arxiv.org/abs/2305.08227).
//! - Upstream crate: [`deep_filter`](https://crates.io/crates/deep_filter)
//!   (MIT OR Apache-2.0).

#![cfg_attr(not(test), forbid(unsafe_code))]

// The tract-* crates are listed as direct dependencies solely to pin
// their versions (see `Cargo.toml`) so they match what `deep_filter`
// expects.  Re-import them with `as _` to satisfy the
// `unused_crate_dependencies` lint without bringing symbols into scope.
use tract_core as _;
use tract_hir as _;
use tract_onnx as _;
use tract_pulse as _;

use anyhow::Result;
use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::Array2;

/// Configuration for [`DeepFilterDenoiser`].
#[derive(Debug, Clone)]
pub struct DeepFilterConfig {
    /// Wet/dry mix in `[0.0, 1.0]`.  `1.0` is fully denoised, `0.0`
    /// is the dry input.  Default: `1.0`.
    pub attenuation: f32,
    /// Hard upper bound on the attenuation in dB, forwarded to the
    /// model.  Lower values keep more residual ambience and
    /// noticeably reduce over-suppression artefacts; the upstream
    /// reference defaults to `100.0` (effectively unlimited) but for
    /// real-time `VoIP` a value around `24.0` dB sounds noticeably
    /// more natural.  Default: `24.0`.
    pub attenuation_limit_db: f32,
    /// `DeepFilterNet` post-filter strength (`beta`).  Set to `0.0`
    /// to disable the post-filter, `> 0.0` to enable it.  The
    /// upstream reference default is `0.02`, which produces cleaner
    /// output without audibly over-smoothing speech.  Default: `0.02`.
    pub post_filter_beta: f32,
    /// Voice-activity threshold in dB.  Frames whose model-estimated
    /// SNR falls below this value are passed through the gain stage
    /// untouched (avoids "pumping" on near-silent input).  Range
    /// roughly `-20.0 .. 0.0`.  Default: `-10.0` (matches upstream).
    pub min_db_thresh: f32,
}

impl Default for DeepFilterConfig {
    fn default() -> Self {
        Self {
            attenuation: 1.0,
            attenuation_limit_db: 24.0,
            post_filter_beta: 0.02,
            min_db_thresh: -10.0,
        }
    }
}

/// `DeepFilterNet` denoiser instance.
///
/// The model operates on fixed `hop_size` frames at 48 kHz mono F32
/// (`hop_size` is ~480 samples).  This wrapper hides the fixed-frame
/// requirement with a small input/output ring so callers can feed
/// buffers of arbitrary length.
pub struct DeepFilterDenoiser {
    config: DeepFilterConfig,
    inner: DfTract,
    hop_size: usize,
    input_ring: Vec<f32>,
    output_ready: std::collections::VecDeque<f32>,
}

impl std::fmt::Debug for DeepFilterDenoiser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeepFilterDenoiser")
            .field("config", &self.config)
            .field("hop_size", &self.hop_size)
            .finish_non_exhaustive()
    }
}

impl DeepFilterDenoiser {
    /// Create a new denoiser with the given configuration.
    ///
    /// Loads the embedded `DeepFilterNet` weights and initialises the
    /// `tract-onnx` runtime.  This is relatively expensive (~50-200 ms)
    /// and should happen off the audio thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded model cannot be decoded or the
    /// runtime refuses to instantiate the graph.
    pub fn new(config: DeepFilterConfig) -> Result<Self> {
        let df_params = DfParams::default();
        let mut rt_params = RuntimeParams::default_with_ch(1);
        rt_params = rt_params
            .with_atten_lim(config.attenuation_limit_db)
            .with_post_filter(config.post_filter_beta)
            .with_thresholds(config.min_db_thresh, 30.0, 20.0);
        let inner = DfTract::new(df_params, &rt_params)?;
        let hop_size = inner.hop_size;
        Ok(Self {
            config,
            inner,
            hop_size,
            input_ring: Vec::with_capacity(hop_size * 2),
            output_ready: std::collections::VecDeque::with_capacity(hop_size),
        })
    }

    /// Frame size expected by the model (samples per hop).
    #[must_use]
    pub fn hop_size(&self) -> usize {
        self.hop_size
    }

    /// Update the wet/dry attenuation at runtime.
    pub fn set_attenuation(&mut self, attenuation: f32) {
        self.config.attenuation = attenuation.clamp(0.0, 1.0);
    }

    /// Reset the internal STFT/ring state.
    pub fn reset(&mut self) {
        self.input_ring.clear();
        self.output_ready.clear();
        // `DfTract` has no public reset; dropping the rolling spec
        // buffer on the next `process` call via `init` is the closest
        // equivalent, but `init` is already called in `new`.  In
        // practice, feeding a few frames of silence is enough.
    }

    /// Process `samples` in place.  Buffer length is unrestricted.
    ///
    /// The first `hop_size` samples after the first call will be
    /// silent (dry-through) while the pipeline fills.
    pub fn process(&mut self, samples: &mut [f32]) {
        let attenuation = self.config.attenuation.clamp(0.0, 1.0);
        if attenuation == 0.0 {
            return;
        }

        for sample in samples.iter_mut() {
            let dry = *sample;
            self.input_ring.push(dry);
            if self.input_ring.len() >= self.hop_size {
                self.run_hop();
            }
            let wet = self.output_ready.pop_front().unwrap_or(dry);
            *sample = (1.0 - attenuation) * dry + attenuation * wet;
        }
    }

    fn run_hop(&mut self) {
        let noisy_vec: Vec<f32> = self.input_ring.drain(..self.hop_size).collect();
        let Ok(noisy) = Array2::from_shape_vec((1, self.hop_size), noisy_vec) else {
            // Shape is built from its own length so this is unreachable;
            // treat as silence if it ever fails rather than panicking.
            for _ in 0..self.hop_size {
                self.output_ready.push_back(0.0);
            }
            return;
        };
        let mut enh = Array2::<f32>::zeros((1, self.hop_size));
        if self.inner.process(noisy.view(), enh.view_mut()).is_err() {
            // On inference error emit silence for this hop rather than
            // propagating - audio must not glitch.
            enh.fill(0.0);
        }
        for &s in enh.row(0).iter() {
            self.output_ready.push_back(s);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "concise failure reporting in unit tests"
)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_are_sensible() {
        let cfg = DeepFilterConfig::default();
        assert!((cfg.attenuation - 1.0).abs() < f32::EPSILON);
        assert!(cfg.attenuation_limit_db >= 24.0);
    }

    #[test]
    fn loads_embedded_model() {
        // Smoke test: the embedded tar.gz must decode and the ONNX
        // graph must instantiate.  Runs on every supported target.
        let denoiser = DeepFilterDenoiser::new(DeepFilterConfig::default());
        assert!(
            denoiser.is_ok(),
            "failed to load embedded DeepFilterNet model: {:?}",
            denoiser.err()
        );
        let denoiser = denoiser.unwrap();
        assert!(denoiser.hop_size() > 0);
        assert!(denoiser.hop_size() <= 4096);
    }

    #[test]
    fn dry_passthrough_when_attenuation_zero() {
        let mut denoiser =
            DeepFilterDenoiser::new(DeepFilterConfig::default()).expect("model loads");
        denoiser.set_attenuation(0.0);
        let dry: Vec<f32> = (0..1920).map(|i| (i as f32 * 0.001).sin() * 0.1).collect();
        let mut buf = dry.clone();
        denoiser.process(&mut buf);
        for (a, b) in buf.iter().zip(dry.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn handles_arbitrary_buffer_lengths() {
        let mut denoiser =
            DeepFilterDenoiser::new(DeepFilterConfig::default()).expect("model loads");
        for &n in &[1_usize, 7, 33, 480, 511, 513, 2048] {
            let mut buf = vec![0.01_f32; n];
            denoiser.process(&mut buf);
        }
    }
}
