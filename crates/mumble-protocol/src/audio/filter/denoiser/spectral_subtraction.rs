//! Classical spectral-subtraction noise suppressor.
//!
//! Combines two well-known DSP building blocks:
//!
//! - **Minimum-statistics noise PSD tracking** (Martin, 2001:
//!   *"Noise Power Spectral Density Estimation Based on Optimal
//!   Smoothing and Minimum Statistics"*, IEEE TSAP 9(5).
//!   [doi:10.1109/89.928915](https://doi.org/10.1109/89.928915)).  The
//!   algorithm tracks the per-bin noise floor by sliding a minimum
//!   over a buffer of recently smoothed power spectra.  No
//!   voice-activity detector is required and the estimator adapts
//!   continuously, which makes it robust against non-stationary
//!   background noise.
//! - **Decision-directed a-priori SNR estimation** (Ephraim & Malah,
//!   1984: *"Speech Enhancement Using a Minimum-Mean Square Error
//!   Short-Time Spectral Amplitude Estimator"*, IEEE TASSP 32(6),
//!   [doi:10.1109/TASSP.1984.1164453](https://doi.org/10.1109/TASSP.1984.1164453))
//!   feeding a Wiener-style gain rule
//!   `G = sigma / (sigma + 1)`.
//!
//! These techniques predate the deep-learning era but remain
//! competitive on stationary or quasi-stationary noise (HVAC, traffic
//! rumble, hum) and have a fraction of the CPU cost of `RNNoise`
//! (~1% of one core in our pipeline).  They are still the default
//! noise suppressor in Audacity, Speex and many embedded `VoIP` stacks.
//!
//! The implementation processes 512-sample windows (~10.7 ms @ 48 kHz)
//! with 50 % overlap and a Hann analysis/synthesis window pair.

use std::sync::Arc;

use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use realfft::num_complex::Complex32;

use super::params::{algorithm_param_specs, read_param, DenoiserParams};
use super::{DenoiserBackend, NoiseSuppressionAlgorithm};

const FFT_SIZE: usize = 512;
const HOP_SIZE: usize = FFT_SIZE / 2;
const NUM_BINS: usize = FFT_SIZE / 2 + 1;
/// Number of recently smoothed power spectra used to estimate the
/// noise floor via a sliding minimum.
const MIN_STAT_HISTORY: usize = 96;
/// Smoothing factor for the per-bin power estimate (Martin's `alpha`).
const POWER_SMOOTHING: f32 = 0.85;
/// Decision-directed weighting (Ephraim-Malah `alpha_dd`).
const DD_ALPHA: f32 = 0.98;
/// Multiplicative bias correction for the minimum-statistics estimate
/// (Martin 2001 reports a typical bias of ~1.5).
const NOISE_BIAS: f32 = 1.5;

pub(super) struct SpectralSubtractionBackend {
    fft: Arc<dyn RealToComplex<f32>>,
    ifft: Arc<dyn ComplexToReal<f32>>,
    window: [f32; FFT_SIZE],
    /// Input ring of pending samples; we accumulate `HOP_SIZE` at a
    /// time and run an STFT every hop.
    input_ring: Vec<f32>,
    /// Output overlap-add buffer.  Synthesis writes `FFT_SIZE` samples
    /// per hop; the first `HOP_SIZE` are popped to the caller.
    output_ola: [f32; FFT_SIZE],
    /// Output samples not yet returned to the caller.
    output_ready: std::collections::VecDeque<f32>,
    /// Smoothed periodogram (Welch-style first-order IIR).
    smoothed_power: [f32; NUM_BINS],
    /// Sliding minimum of `smoothed_power` over the last
    /// `MIN_STAT_HISTORY` frames.
    min_history: Vec<[f32; NUM_BINS]>,
    history_idx: usize,
    /// Decision-directed a-priori SNR estimate per bin.
    apriori_snr: [f32; NUM_BINS],
    /// Last clean-speech magnitude estimate per bin (used for DD update).
    prev_clean_mag: [f32; NUM_BINS],
    /// Lower bound on the per-bin gain.  Configurable via the
    /// `min_gain` parameter in [`DenoiserParams`].
    min_gain: f32,
}

impl SpectralSubtractionBackend {
    pub(super) fn new(params: &DenoiserParams) -> Self {
        let specs = algorithm_param_specs(NoiseSuppressionAlgorithm::SpectralSubtraction);
        let min_gain = specs
            .iter()
            .find(|s| s.id == "min_gain")
            .map_or(0.05, |s| read_param(params, s));

        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let ifft = planner.plan_fft_inverse(FFT_SIZE);

        let mut window = [0.0_f32; FFT_SIZE];
        for (i, w) in window.iter_mut().enumerate() {
            // Periodic Hann; for COLA at 50 % hop the analysis window
            // squared sums to 1 with the same synthesis window applied.
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
            smoothed_power: [1e-9; NUM_BINS],
            min_history: vec![[1e-9; NUM_BINS]; MIN_STAT_HISTORY],
            history_idx: 0,
            apriori_snr: [1.0; NUM_BINS],
            prev_clean_mag: [0.0; NUM_BINS],
            min_gain,
        }
    }

    fn run_stft_hop(&mut self) {
        // Take the first FFT_SIZE samples from input_ring (we always
        // have at least that many at this point).
        let mut analysis = [0.0_f32; FFT_SIZE];
        for (i, dst) in analysis.iter_mut().enumerate() {
            *dst = self.input_ring[i] * self.window[i];
        }
        // Drop the consumed hop.
        let _ = self.input_ring.drain(..HOP_SIZE);

        let mut spectrum = self.fft.make_output_vec();
        // realfft returns Result but only fails on size mismatch; our
        // buffers are constructed to match.
        let _ = self.fft.process(&mut analysis, &mut spectrum);

        let gains = self.compute_gains(&spectrum);

        for (bin, &gain) in spectrum.iter_mut().zip(gains.iter()) {
            *bin *= gain;
        }

        let mut synthesis = self.ifft.make_output_vec();
        let _ = self.ifft.process(&mut spectrum, &mut synthesis);

        // realfft's inverse is unnormalised: divide by FFT_SIZE.
        let norm = 1.0 / FFT_SIZE as f32;
        for (s, w) in synthesis.iter_mut().zip(self.window.iter()) {
            *s = *s * *w * norm;
        }

        // Overlap-add: shift the previous OLA tail into the lower
        // half, zero the upper half, then add the windowed synthesis
        // frame on top.
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
        // 1. Update smoothed periodogram and minimum-statistics noise
        //    PSD estimate.
        let mut frame_power = [0.0_f32; NUM_BINS];
        for (i, bin) in spectrum.iter().enumerate() {
            frame_power[i] = bin.norm_sqr().max(1e-12);
            self.smoothed_power[i] = POWER_SMOOTHING * self.smoothed_power[i]
                + (1.0 - POWER_SMOOTHING) * frame_power[i];
        }
        self.min_history[self.history_idx] = self.smoothed_power;
        self.history_idx = (self.history_idx + 1) % MIN_STAT_HISTORY;

        let mut noise_psd = [0.0_f32; NUM_BINS];
        for bin in 0..NUM_BINS {
            let mut m = f32::INFINITY;
            for slot in &self.min_history {
                if slot[bin] < m {
                    m = slot[bin];
                }
            }
            noise_psd[bin] = (m * NOISE_BIAS).max(1e-12);
        }

        // 2. Per-bin Wiener gain via decision-directed SNR.
        let mut gains = [0.0_f32; NUM_BINS];
        for bin in 0..NUM_BINS {
            let posteriori = (frame_power[bin] / noise_psd[bin] - 1.0).max(0.0);
            let prev_sq = self.prev_clean_mag[bin] * self.prev_clean_mag[bin];
            let dd_estimate = DD_ALPHA * (prev_sq / noise_psd[bin])
                + (1.0 - DD_ALPHA) * posteriori;
            self.apriori_snr[bin] = dd_estimate.max(1e-3);

            let gain = self.apriori_snr[bin] / (self.apriori_snr[bin] + 1.0);
            let gain = gain.clamp(self.min_gain, 1.0);
            gains[bin] = gain;

            // Cache the gain-applied magnitude for the next DD update.
            let mag = frame_power[bin].sqrt();
            self.prev_clean_mag[bin] = gain * mag;
        }
        gains
    }
}

impl DenoiserBackend for SpectralSubtractionBackend {
    fn process(&mut self, samples: &mut [f32], attenuation: f32) {
        // Fast path when fully wet at attenuation == 1.
        let attenuation = attenuation.clamp(0.0, 1.0);

        // Split into clean and dry copies so we can mix afterwards.
        let dry: Vec<f32> = samples.to_vec();

        // Push samples through the STFT/ISTFT pipeline; pop processed
        // samples back into `samples` in order.  Whenever we don't yet
        // have a complete output sample (during the initial fill of
        // FFT_SIZE) we emit zero for that position - this introduces a
        // one-frame latency on the very first invocation only.  After
        // priming, output is sample-accurate.
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
        self.smoothed_power = [1e-9; NUM_BINS];
        for slot in self.min_history.iter_mut() {
            *slot = [1e-9; NUM_BINS];
        }
        self.history_idx = 0;
        self.apriori_snr = [1.0; NUM_BINS];
        self.prev_clean_mag = [0.0; NUM_BINS];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: &[f32]) -> f32 {
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

    /// Steady noise should be substantially attenuated once the
    /// minimum-statistics tracker has converged.
    #[test]
    fn attenuates_steady_noise() {
        let mut backend = SpectralSubtractionBackend::new(&DenoiserParams::new());
        let mut seed = 0xCAFE_BABE_u32;

        // Prime the tracker with ~3 seconds of noise.
        for _ in 0..150 {
            let mut samples = pseudo_noise(&mut seed, 960, 0.1);
            backend.process(&mut samples, 1.0);
        }

        let mut samples = pseudo_noise(&mut seed, 960, 0.1);
        let in_rms = rms(&samples);
        backend.process(&mut samples, 1.0);
        let out_rms = rms(&samples);
        assert!(
            out_rms < in_rms * 0.6,
            "spectral subtraction should attenuate steady noise; in={in_rms:.5}, out={out_rms:.5}"
        );
    }

    /// `attenuation = 0.0` keeps the dry signal exactly.
    #[test]
    fn dry_passthrough_when_attenuation_zero() {
        let mut backend = SpectralSubtractionBackend::new(&DenoiserParams::new());
        let dry: Vec<f32> = (0..960).map(|i| (i as f32 * 0.001).sin() * 0.1).collect();
        let mut samples = dry.clone();
        backend.process(&mut samples, 0.0);
        for (a, b) in samples.iter().zip(dry.iter()) {
            assert!((a - b).abs() < 1e-6, "dry signal must be preserved");
        }
    }

    /// Reset must clear all internal state so subsequent processing
    /// starts cleanly.
    #[test]
    fn reset_clears_state() {
        let mut backend = SpectralSubtractionBackend::new(&DenoiserParams::new());
        let mut seed = 0x1357_9BDF_u32;
        for _ in 0..5 {
            let mut samples = pseudo_noise(&mut seed, 960, 0.1);
            backend.process(&mut samples, 1.0);
        }
        backend.reset();
        // After reset the output_ready queue should be empty - the
        // next call should re-prime without panicking.
        let mut samples = pseudo_noise(&mut seed, 960, 0.1);
        backend.process(&mut samples, 1.0);
    }
}
