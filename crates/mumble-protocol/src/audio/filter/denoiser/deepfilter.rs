//! `DeepFilterNet` backend, gated behind the `deepfilternet-denoiser`
//! cargo feature.  Delegates to the standalone
//! [`fancy_denoiser_deepfilter`] crate.

use fancy_denoiser_deepfilter::{DeepFilterConfig, DeepFilterDenoiser};

use super::params::{algorithm_param_specs, read_param, DenoiserParams};
use super::{DenoiserBackend, NoiseSuppressionAlgorithm};

/// Thread-ownership wrapper around [`DeepFilterDenoiser`].
///
/// `DeepFilterDenoiser` transitively owns `tract`'s `Rc<Tensor>` and
/// `Box<dyn OpState>` which are `!Send` by default.  The filter
/// chain is moved once to the audio capture task and stays there for
/// its whole lifetime, so exclusive-ownership `Send` is sound.
struct ThreadOwnedDenoiser(DeepFilterDenoiser);

// SAFETY: `ThreadOwnedDenoiser` owns its inner `DeepFilterDenoiser`
// exclusively and the denoiser is never cloned or shared across
// threads.  Ownership is transferred once (to the audio task) and all
// subsequent accesses happen on that single thread through `&mut`.
#[allow(
    unsafe_code,
    reason = "DfTract's internal Rc is never aliased across threads; see ThreadOwnedDenoiser docs"
)]
unsafe impl Send for ThreadOwnedDenoiser {}

pub(super) struct DeepFilterBackend {
    inner: Option<ThreadOwnedDenoiser>,
    #[allow(dead_code, reason = "kept for future diagnostics / logging")]
    hop_size: usize,
}

fn build_config(params: &DenoiserParams) -> DeepFilterConfig {
    let specs = algorithm_param_specs(NoiseSuppressionAlgorithm::DeepFilterNet);
    let lookup = |id: &str, fallback: f32| {
        specs
            .iter()
            .find(|s| s.id == id)
            .map_or(fallback, |s| read_param(params, s))
    };
    DeepFilterConfig {
        attenuation: 1.0,
        attenuation_limit_db: lookup("atten_lim_db", 24.0),
        post_filter_beta: lookup("post_filter_beta", 0.02),
        min_db_thresh: lookup("min_db_thresh", -10.0),
    }
}

impl DeepFilterBackend {
    pub(super) fn new(params: &DenoiserParams) -> Self {
        let cfg = build_config(params);
        match DeepFilterDenoiser::new(cfg) {
            Ok(inner) => {
                let hop_size = inner.hop_size();
                Self {
                    inner: Some(ThreadOwnedDenoiser(inner)),
                    hop_size,
                }
            }
            Err(err) => {
                tracing::error!(
                    "failed to load DeepFilterNet model, falling back to pass-through: {err}"
                );
                Self {
                    inner: None,
                    hop_size: 480,
                }
            }
        }
    }
}

impl DenoiserBackend for DeepFilterBackend {
    fn process(&mut self, samples: &mut [f32], attenuation: f32) {
        if let Some(d) = self.inner.as_mut() {
            d.0.set_attenuation(attenuation);
            d.0.process(samples);
        }
    }

    fn reset(&mut self) {
        if let Some(d) = self.inner.as_mut() {
            d.0.reset();
        }
    }
}

#[allow(dead_code, reason = "used by #[cfg(test)] tests in sibling modules")]
pub(super) fn default_hop_size() -> usize {
    480
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_without_panic() {
        let mut backend = DeepFilterBackend::new(&DenoiserParams::new());
        // Whether the model loaded or not, process() must not panic.
        let mut buf = vec![0.01_f32; 960];
        backend.process(&mut buf, 1.0);
        assert!(backend.hop_size > 0);
    }

    #[test]
    fn build_config_reads_user_params() {
        let mut params = DenoiserParams::new();
        let _ = params.insert("atten_lim_db".into(), 18.0);
        let _ = params.insert("post_filter_beta".into(), 0.03);
        let cfg = build_config(&params);
        assert!((cfg.attenuation_limit_db - 18.0).abs() < f32::EPSILON);
        assert!((cfg.post_filter_beta - 0.03).abs() < f32::EPSILON);
    }
}

