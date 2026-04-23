//! `RNNoise` GRU-based denoiser backend.

use super::DenoiserBackend;

#[cfg(feature = "rnnoise-denoiser")]
use nnnoiseless::DenoiseState;

#[cfg(feature = "rnnoise-denoiser")]
pub(super) struct RnnoiseBackend {
    state: Box<DenoiseState<'static>>,
    /// True until the very first 480-sample chunk has been processed:
    /// `RNNoise`'s first output contains fade-in artefacts that we
    /// suppress by carrying the dry signal instead.
    warming_up: bool,
}

#[cfg(feature = "rnnoise-denoiser")]
impl RnnoiseBackend {
    pub(super) fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            warming_up: true,
        }
    }
}

#[cfg(feature = "rnnoise-denoiser")]
impl DenoiserBackend for RnnoiseBackend {
    /// Apply `RNNoise` to `samples` in place, blending the result with
    /// the original signal according to `attenuation`.
    ///
    /// Processes all complete 480-sample chunks; any trailing partial
    /// chunk is left untouched (this never happens with our standard
    /// 10/20/40/60 ms frame sizes).
    fn process(&mut self, samples: &mut [f32], attenuation: f32) {
        const FRAME: usize = DenoiseState::FRAME_SIZE;
        // RNNoise expects f32 in the i16 range, NOT [-1.0, 1.0].
        const SCALE_UP: f32 = i16::MAX as f32;
        const SCALE_DOWN: f32 = 1.0 / SCALE_UP;

        let mut in_buf = [0.0_f32; FRAME];
        let mut out_buf = [0.0_f32; FRAME];

        for chunk in samples.chunks_exact_mut(FRAME) {
            for (dst, &src) in in_buf.iter_mut().zip(chunk.iter()) {
                *dst = src * SCALE_UP;
            }
            // Returned value is the per-frame voice-activity probability;
            // we currently rely on the downstream noise gate for VAD.
            let _voice_activity = self.state.process_frame(&mut out_buf, &in_buf);

            if self.warming_up {
                self.warming_up = false;
                continue;
            }

            for (dst, (&dry, &wet)) in chunk
                .iter_mut()
                .zip(in_buf.iter().zip(out_buf.iter()))
            {
                let dry_norm = dry * SCALE_DOWN;
                let wet_norm = wet * SCALE_DOWN;
                *dst = (1.0 - attenuation) * dry_norm + attenuation * wet_norm;
            }
        }
    }

    fn reset(&mut self) {
        self.state = DenoiseState::new();
        self.warming_up = true;
    }
}

#[cfg(all(test, feature = "rnnoise-denoiser"))]
mod tests {
    use super::*;

    fn rms(samples: &[f32]) -> f32 {
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

    /// Pseudo-random noise should be at least somewhat attenuated.
    #[test]
    fn rnnoise_attenuates_white_noise() {
        let mut backend = RnnoiseBackend::new();

        let mut seed: u32 = 0x1234_5678;
        let mut next_noise = |n: usize| -> Vec<f32> {
            (0..n)
                .map(|_| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    (((seed >> 16) & 0x7FFF) as f32 / 32_768.0 - 0.5) * 0.1
                })
                .collect()
        };

        let mut last_in_rms = 0.0;
        let mut last_out_rms = 0.0;
        for _ in 0..50 {
            let mut samples = next_noise(960);
            last_in_rms = rms(&samples);
            backend.process(&mut samples, 1.0);
            last_out_rms = rms(&samples);
        }

        assert!(
            last_out_rms < last_in_rms * 0.85,
            "RNNoise should attenuate pseudo-random noise; in={last_in_rms:.5}, out={last_out_rms:.5}"
        );
    }
}
