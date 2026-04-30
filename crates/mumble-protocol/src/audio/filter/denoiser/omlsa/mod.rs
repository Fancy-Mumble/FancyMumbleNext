//! OMLSA + IMCRA denoiser backend.
//!
//! Inlined implementation of:
//!
//! - I. Cohen and B. Berdugo, *"Speech enhancement for non-stationary
//!   noise environments"*, Signal Processing 81(11), 2001, 2403-2418
//!   - the **Optimally Modified Log-Spectral Amplitude (OMLSA)**
//!     estimator.
//! - I. Cohen, *"Noise spectrum estimation in adverse environments:
//!   Improved minima controlled recursive averaging"*, IEEE TSAP
//!   11(5), 2003 - the **Improved Minima-Controlled Recursive
//!   Averaging (IMCRA)** noise PSD estimator.
//!
//! See the module-level design doc
//! [`crates/mumble-protocol/doc/denoiser-roadmap.md`](../../../../doc/denoiser-roadmap.md)
//! for the rationale behind each backend.

use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

mod imcra;

use imcra::Imcra;

use super::params::{algorithm_param_specs, read_param, DenoiserParams};
use super::{DenoiserBackend, NoiseSuppressionAlgorithm};

const FFT_SIZE: usize = 512;
const HOP_SIZE: usize = FFT_SIZE / 2;
const NUM_BINS: usize = FFT_SIZE / 2 + 1;
/// Decision-directed weighting from Ephraim & Malah (1984).
const DD_ALPHA: f32 = 0.92;
/// Lower bound on the a-priori SNR to keep the gain numerically stable.
const APRIORI_FLOOR: f32 = 1e-3;

pub(super) struct OmlsaBackend {
    fft: Arc<dyn RealToComplex<f32>>,
    ifft: Arc<dyn ComplexToReal<f32>>,
    window: [f32; FFT_SIZE],
    input_ring: Vec<f32>,
    output_ola: [f32; FFT_SIZE],
    output_ready: std::collections::VecDeque<f32>,
    apriori_snr: [f32; NUM_BINS],
    prev_clean_mag: [f32; NUM_BINS],
    noise_tracker: Imcra,
    /// Lower bound on the per-bin gain.  Configurable via the
    /// `min_gain` parameter in [`DenoiserParams`].
    min_gain: f32,
}

impl OmlsaBackend {
    pub(super) fn new(params: &DenoiserParams) -> Self {
        let specs = algorithm_param_specs(NoiseSuppressionAlgorithm::OmlsaImcra);
        let min_gain = specs
            .iter()
            .find(|s| s.id == "min_gain")
            .map_or(0.05, |s| read_param(params, s));
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let ifft = planner.plan_fft_inverse(FFT_SIZE);

        let mut window = [0.0_f32; FFT_SIZE];
        for (i, w) in window.iter_mut().enumerate() {
            let phase = std::f32::consts::TAU * i as f32 / FFT_SIZE as f32;
            *w = 0.5 - 0.5 * phase.cos();
        }

        Self {
            fft,
            ifft,
            window,
            input_ring: Vec::with_capacity(FFT_SIZE * 2),
            output_ola: [0.0; FFT_SIZE],
            output_ready: std::collections::VecDeque::with_capacity(FFT_SIZE),
            apriori_snr: [1.0; NUM_BINS],
            prev_clean_mag: [0.0; NUM_BINS],
            noise_tracker: Imcra::new(NUM_BINS),
            min_gain,
        }
    }

    fn run_stft_hop(&mut self) {
        let mut analysis = [0.0_f32; FFT_SIZE];
        for (i, dst) in analysis.iter_mut().enumerate() {
            *dst = self.input_ring[i] * self.window[i];
        }
        let _ = self.input_ring.drain(..HOP_SIZE);

        let mut spectrum = self.fft.make_output_vec();
        let _ = self.fft.process(&mut analysis, &mut spectrum);

        let gains = self.compute_gains(&spectrum);
        for (bin, &gain) in spectrum.iter_mut().zip(gains.iter()) {
            *bin *= gain;
        }

        let mut synthesis = self.ifft.make_output_vec();
        let _ = self.ifft.process(&mut spectrum, &mut synthesis);

        let norm = 1.0 / FFT_SIZE as f32;
        for (s, w) in synthesis.iter_mut().zip(self.window.iter()) {
            *s = *s * *w * norm;
        }

        self.output_ola.copy_within(HOP_SIZE..FFT_SIZE, 0);
        self.output_ola[HOP_SIZE..FFT_SIZE].fill(0.0);
        for (out, syn) in self.output_ola.iter_mut().zip(synthesis.iter()) {
            *out += *syn;
        }
        for &s in self.output_ola[..HOP_SIZE].iter() {
            self.output_ready.push_back(s);
        }
    }

    fn compute_gains(&mut self, spectrum: &[Complex32]) -> [f32; NUM_BINS] {
        let mut frame_power = [0.0_f32; NUM_BINS];
        for (p, bin) in frame_power.iter_mut().zip(spectrum.iter()) {
            *p = bin.norm_sqr().max(1e-12);
        }

        self.noise_tracker.update(&frame_power);
        let noise_psd = self.noise_tracker.noise_psd();
        let speech_prob = self.noise_tracker.speech_presence_probability();

        let mut gains = [0.0_f32; NUM_BINS];
        for bin in 0..NUM_BINS {
            let n = noise_psd[bin].max(1e-12);
            let posteriori = (frame_power[bin] / n - 1.0).max(0.0);
            let prev_sq = self.prev_clean_mag[bin] * self.prev_clean_mag[bin];
            let dd = DD_ALPHA * (prev_sq / n) + (1.0 - DD_ALPHA) * posteriori;
            self.apriori_snr[bin] = dd.max(APRIORI_FLOOR);

            let xi = self.apriori_snr[bin];
            let v = xi * (frame_power[bin] / n) / (1.0 + xi);
            let g_h1 = (xi / (1.0 + xi)) * exp_int_approx(v).exp();
            let p = speech_prob[bin];
            let gain_log = p * g_h1.ln() + (1.0 - p) * self.min_gain.ln();
            let gain = gain_log.exp().clamp(self.min_gain, 1.0);
            gains[bin] = gain;

            self.prev_clean_mag[bin] = gain * frame_power[bin].sqrt();
        }
        gains
    }
}

impl DenoiserBackend for OmlsaBackend {
    fn process(&mut self, samples: &mut [f32], attenuation: f32) {
        let attenuation = attenuation.clamp(0.0, 1.0);
        let dry: Vec<f32> = samples.to_vec();
        for (i, sample) in samples.iter_mut().enumerate() {
            self.input_ring.push(dry[i]);
            while self.input_ring.len() >= FFT_SIZE {
                self.run_stft_hop();
            }
            let wet = self.output_ready.pop_front().unwrap_or(0.0);
            *sample = (1.0 - attenuation) * dry[i] + attenuation * wet;
        }
    }

    fn reset(&mut self) {
        self.input_ring.clear();
        self.output_ola = [0.0; FFT_SIZE];
        self.output_ready.clear();
        self.apriori_snr = [1.0; NUM_BINS];
        self.prev_clean_mag = [0.0; NUM_BINS];
        self.noise_tracker.reset();
    }
}

/// Approximation of `0.5 * E1(v)` where `E1` is the exponential
/// integral, used in the OMLSA gain (Cohen 2001 eq. 17).
fn exp_int_approx(v: f32) -> f32 {
    let v = v.max(1e-6);
    if v < 0.1 {
        const EULER_MASCHERONI: f32 = 0.577_215_7;
        -0.5 * (EULER_MASCHERONI + v.ln())
    } else {
        0.5 * (-v).exp() / (v + 0.5 + 1.0 / (v + 1.5))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(s: &[f32]) -> f32 {
        let sum_sq: f32 = s.iter().map(|x| x * x).sum();
        (sum_sq / s.len() as f32).sqrt()
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
    fn omlsa_attenuates_steady_noise() {
        let mut backend = OmlsaBackend::new(&DenoiserParams::new());
        let mut seed: u32 = 0xBEEF_CAFE;
        for _ in 0..150 {
            let mut buf = pseudo_noise(&mut seed, 960, 0.1);
            backend.process(&mut buf, 1.0);
        }
        let mut buf = pseudo_noise(&mut seed, 960, 0.1);
        let in_rms = rms(&buf);
        backend.process(&mut buf, 1.0);
        let out_rms = rms(&buf);
        assert!(
            out_rms < in_rms * 0.7,
            "OMLSA should attenuate steady noise; in={in_rms:.5}, out={out_rms:.5}"
        );
    }

    #[test]
    fn omlsa_dry_passthrough_when_attenuation_zero() {
        let mut backend = OmlsaBackend::new(&DenoiserParams::new());
        let dry: Vec<f32> = (0..1920).map(|i| (i as f32 * 0.001).sin() * 0.1).collect();
        let mut buf = dry.clone();
        backend.process(&mut buf, 0.0);
        for (a, b) in buf.iter().zip(dry.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn omlsa_reset_clears_state() {
        let mut backend = OmlsaBackend::new(&DenoiserParams::new());
        let mut seed: u32 = 0xCAFE;
        for _ in 0..10 {
            let mut buf = pseudo_noise(&mut seed, 960, 0.1);
            backend.process(&mut buf, 1.0);
        }
        backend.reset();
        let mut buf = pseudo_noise(&mut seed, 960, 0.1);
        backend.process(&mut buf, 1.0);
    }
}
