//! IMCRA - Improved Minima Controlled Recursive Averaging.
//!
//! Reference: I. Cohen, *"Noise spectrum estimation in adverse
//! environments: Improved minima controlled recursive averaging"*,
//! IEEE Transactions on Speech and Audio Processing 11(5), 2003.

const ALPHA_S: f32 = 0.8;
const ALPHA_D: f32 = 0.85;
const NUM_SUBWINDOWS: usize = 8;
const SUBWINDOW_LEN: usize = 15;
const B_MIN: f32 = 1.66;
const GAMMA_THRESH: f32 = 4.6;
const ALPHA_P: f32 = 0.2;

#[derive(Debug)]
pub(super) struct Imcra {
    num_bins: usize,
    smoothed: Vec<f32>,
    subwindow_min: Vec<f32>,
    subwindow_history: Vec<Vec<f32>>,
    history_idx: usize,
    subwindow_counter: usize,
    noise_psd: Vec<f32>,
    speech_prob: Vec<f32>,
    initialised: bool,
}

impl Imcra {
    pub(super) fn new(num_bins: usize) -> Self {
        let init = vec![1e-8_f32; num_bins];
        Self {
            num_bins,
            smoothed: init.clone(),
            subwindow_min: vec![f32::INFINITY; num_bins],
            subwindow_history: vec![init.clone(); NUM_SUBWINDOWS],
            history_idx: 0,
            subwindow_counter: 0,
            noise_psd: init,
            speech_prob: vec![0.0; num_bins],
            initialised: false,
        }
    }

    pub(super) fn reset(&mut self) {
        self.smoothed.iter_mut().for_each(|s| *s = 1e-8);
        self.subwindow_min.iter_mut().for_each(|s| *s = f32::INFINITY);
        for sw in &mut self.subwindow_history {
            sw.iter_mut().for_each(|s| *s = 1e-8);
        }
        self.history_idx = 0;
        self.subwindow_counter = 0;
        self.noise_psd.iter_mut().for_each(|s| *s = 1e-8);
        self.speech_prob.iter_mut().for_each(|s| *s = 0.0);
        self.initialised = false;
    }

    pub(super) fn noise_psd(&self) -> &[f32] {
        &self.noise_psd
    }

    pub(super) fn speech_presence_probability(&self) -> &[f32] {
        &self.speech_prob
    }

    pub(super) fn update(&mut self, frame_power: &[f32]) {
        debug_assert_eq!(frame_power.len(), self.num_bins);

        if !self.initialised {
            self.noise_psd.copy_from_slice(frame_power);
            self.smoothed.copy_from_slice(frame_power);
            for sw in &mut self.subwindow_history {
                sw.copy_from_slice(frame_power);
            }
            self.subwindow_min.copy_from_slice(frame_power);
            self.initialised = true;
            return;
        }

        for (s, &p) in self.smoothed.iter_mut().zip(frame_power.iter()) {
            *s = ALPHA_S * *s + (1.0 - ALPHA_S) * p;
        }

        for (m, &s) in self.subwindow_min.iter_mut().zip(self.smoothed.iter()) {
            if s < *m {
                *m = s;
            }
        }

        self.subwindow_counter += 1;
        if self.subwindow_counter >= SUBWINDOW_LEN {
            self.subwindow_history[self.history_idx].copy_from_slice(&self.subwindow_min);
            self.history_idx = (self.history_idx + 1) % NUM_SUBWINDOWS;
            self.subwindow_counter = 0;
            self.subwindow_min.iter_mut().for_each(|s| *s = f32::INFINITY);
        }

        let mut s_min = vec![f32::INFINITY; self.num_bins];
        for sw in &self.subwindow_history {
            for (m, &v) in s_min.iter_mut().zip(sw.iter()) {
                if v < *m {
                    *m = v;
                }
            }
        }
        for (m, &v) in s_min.iter_mut().zip(self.subwindow_min.iter()) {
            if v < *m {
                *m = v;
            }
        }

        for (p, (&s, &m)) in self
            .speech_prob
            .iter_mut()
            .zip(self.smoothed.iter().zip(s_min.iter()))
        {
            let s_r = s / (B_MIN * m.max(1e-12));
            let indicator = if s_r > GAMMA_THRESH { 1.0 } else { 0.0 };
            *p = ALPHA_P * *p + (1.0 - ALPHA_P) * indicator;
        }

        for ((n, &p), &fp) in self
            .noise_psd
            .iter_mut()
            .zip(self.speech_prob.iter())
            .zip(frame_power.iter())
        {
            let alpha_tilde = ALPHA_D + (1.0 - ALPHA_D) * p;
            *n = alpha_tilde * *n + (1.0 - alpha_tilde) * fp;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imcra_converges_to_white_noise_floor() {
        let mut tracker = Imcra::new(33);
        let frame = [0.04_f32; 33];
        for _ in 0..400 {
            tracker.update(&frame);
        }
        for &v in tracker.noise_psd() {
            assert!(
                (v - 0.04).abs() < 0.01,
                "expected noise PSD ~0.04, got {v}"
            );
        }
    }

    #[test]
    fn imcra_freezes_during_speech_burst() {
        let mut tracker = Imcra::new(33);
        let quiet = [0.001_f32; 33];
        for _ in 0..400 {
            tracker.update(&quiet);
        }
        let baseline = tracker.noise_psd().to_vec();
        let burst = [0.1_f32; 33];
        for _ in 0..20 {
            tracker.update(&burst);
        }
        for (i, (&new, &old)) in tracker.noise_psd().iter().zip(baseline.iter()).enumerate() {
            assert!(
                new < old * 5.0,
                "bin {i}: noise PSD ran away during speech burst (old={old:.4}, new={new:.4})"
            );
        }
    }
}
