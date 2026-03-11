//! Audio quality integration tests.
//!
//! These tests generate known signals (sine waves), push them through
//! individual pipeline stages and the full encode→decode roundtrip,
//! then analyse the output with FFT to verify spectral purity, level
//! stability, and absence of artefacts.
//!
//! ```sh
//! cargo test --package mumble-protocol --test audio_quality --features opus-codec
//! ```

use mumble_protocol::audio::decoder::{AudioDecoder, OpusDecoder};
use mumble_protocol::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
use mumble_protocol::audio::filter::automatic_gain::AutomaticGainControl;
use mumble_protocol::audio::filter::automatic_gain::AgcConfig;
use mumble_protocol::audio::filter::noise_gate::{NoiseGate, NoiseGateConfig};
use mumble_protocol::audio::filter::AudioFilter;
use mumble_protocol::audio::sample::{AudioFormat, AudioFrame};

const SAMPLE_RATE: u32 = 48_000;
const FRAME_SIZE: usize = 960; // 20 ms @ 48 kHz

// ────────────────────────────────────────────────────────────────────
//  Helpers
// ────────────────────────────────────────────────────────────────────

/// Generate a mono 48 kHz sine wave frame at given frequency and amplitude.
fn sine_frame(freq_hz: f32, amplitude: f32, frame_size: usize, phase: f32) -> (AudioFrame, f32) {
    let mut samples = Vec::with_capacity(frame_size);
    let mut p = phase;
    let dt = freq_hz / SAMPLE_RATE as f32;
    for _ in 0..frame_size {
        samples.push(amplitude * (2.0 * std::f32::consts::PI * p).sin());
        p += dt;
    }
    // Keep phase in [0, 1) for continuity across frames.
    let next_phase = p % 1.0;

    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_ne_bytes()).collect();
    let frame = AudioFrame {
        data,
        format: AudioFormat::MONO_48KHZ_F32,
        sequence: 0,
        is_silent: false,
    };
    (frame, next_phase)
}

/// Generate multiple contiguous sine frames.
fn sine_frames(freq_hz: f32, amplitude: f32, count: usize) -> Vec<AudioFrame> {
    let mut frames = Vec::with_capacity(count);
    let mut phase = 0.0f32;
    for _ in 0..count {
        let (f, p) = sine_frame(freq_hz, amplitude, FRAME_SIZE, phase);
        frames.push(f);
        phase = p;
    }
    frames
}

/// Compute the RMS of a f32 sample slice.
fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

// ────────────────────────────────────────────────────────────────────
//  Minimal FFT (Cooley-Tukey radix-2 DIT)
// ────────────────────────────────────────────────────────────────────

/// Simple in-place radix-2 FFT.  `n` must be a power of two.
fn fft(real: &mut [f32], imag: &mut [f32]) {
    let n = real.len();
    assert_eq!(n, imag.len());
    assert!(n.is_power_of_two(), "FFT length must be a power of two");

    // Bit-reversal permutation
    let mut j: usize = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            real.swap(i, j);
            imag.swap(i, j);
        }
    }

    // Butterfly stages
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let angle = -2.0 * std::f32::consts::PI / len as f32;
        let wn_r = angle.cos();
        let wn_i = angle.sin();

        let mut start = 0;
        while start < n {
            let mut w_r = 1.0f32;
            let mut w_i = 0.0f32;
            for k in 0..half {
                let a = start + k;
                let b = start + k + half;
                let tr = w_r * real[b] - w_i * imag[b];
                let ti = w_r * imag[b] + w_i * real[b];
                real[b] = real[a] - tr;
                imag[b] = imag[a] - ti;
                real[a] += tr;
                imag[a] += ti;
                let new_wr = w_r * wn_r - w_i * wn_i;
                let new_wi = w_r * wn_i + w_i * wn_r;
                w_r = new_wr;
                w_i = new_wi;
            }
            start += len;
        }
        len <<= 1;
    }
}

/// Compute the magnitude spectrum from a slice of f32 samples.
/// Applies a Hann window to reduce spectral leakage, then pads/truncates
/// to the next power of two.  Returns (magnitudes, `bin_width_hz`).
fn magnitude_spectrum(samples: &[f32]) -> (Vec<f32>, f32) {
    let n = samples.len().next_power_of_two();
    let mut real = vec![0.0f32; n];
    let mut imag = vec![0.0f32; n];
    // Apply Hann window to reduce spectral leakage from non-integer
    // bin frequencies and truncation artefacts.
    let len = samples.len();
    for (i, &s) in samples.iter().enumerate() {
        let w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / len as f32).cos());
        real[i] = s * w;
    }
    fft(&mut real, &mut imag);
    let mags: Vec<f32> = real
        .iter()
        .zip(imag.iter())
        .map(|(&r, &i)| (r * r + i * i).sqrt() / n as f32)
        .collect();
    let bin_hz = SAMPLE_RATE as f32 / n as f32;
    (mags, bin_hz)
}

/// Find the frequency (Hz) of the strongest peak in the first half of the
/// spectrum (below Nyquist).
fn dominant_frequency(samples: &[f32]) -> (f32, f32) {
    let (mags, bin_hz) = magnitude_spectrum(samples);
    let half = mags.len() / 2;
    let (max_bin, &max_mag) = mags[1..half]
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .unwrap();
    let freq = (max_bin + 1) as f32 * bin_hz;
    (freq, max_mag)
}

/// Total harmonic distortion + noise (THD+N), approximated.
/// We assume `fundamental_bin` ± 2 bins carry the fundamental energy;
/// everything else is distortion / noise.
fn thd_n(samples: &[f32], fundamental_hz: f32) -> f32 {
    let (mags, bin_hz) = magnitude_spectrum(samples);
    let half = mags.len() / 2;
    let fund_bin = (fundamental_hz / bin_hz).round() as usize;
    let margin = 8; // ±8 bins around fundamental (accounts for Hann window main lobe)

    let mut signal_power = 0.0f32;
    let mut noise_power = 0.0f32;

    for (i, &mag) in mags.iter().enumerate().take(half).skip(1) {
        let power = mag * mag;
        if i.abs_diff(fund_bin) <= margin {
            signal_power += power;
        } else {
            noise_power += power;
        }
    }

    if signal_power < 1e-12 {
        return 1.0; // all noise
    }
    (noise_power / signal_power).sqrt()
}

// ────────────────────────────────────────────────────────────────────
//  Tests: Opus encode → decode roundtrip
// ────────────────────────────────────────────────────────────────────

#[test]
fn opus_roundtrip_preserves_frequency() {
    let freq = 440.0;
    let amplitude = 0.3;
    let frames = sine_frames(freq, amplitude, 50); // 1 second

    let config = OpusEncoderConfig::default();
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let mut decoder = OpusDecoder::new(fmt).unwrap();

    // Encode then decode all frames, concatenate output samples.
    let mut output_samples: Vec<f32> = Vec::new();
    for frame in &frames {
        let packet = encoder.encode(frame).unwrap();
        let decoded = decoder.decode(&packet).unwrap();
        output_samples.extend_from_slice(decoded.as_f32_samples());
    }

    // Skip the first few frames (codec warm-up / transient).
    let skip = FRAME_SIZE * 5;
    let steady = &output_samples[skip..];

    // Dominant frequency should be ≈ 440 Hz.
    let (dom_freq, _) = dominant_frequency(steady);
    let freq_error = (dom_freq - freq).abs();
    assert!(
        freq_error < 10.0,
        "Dominant frequency should be ~{freq} Hz, got {dom_freq} Hz (error {freq_error} Hz)"
    );
}

#[test]
fn opus_roundtrip_thd_n_acceptable() {
    let freq = 1000.0;
    let amplitude = 0.3;
    let frames = sine_frames(freq, amplitude, 50);

    let config = OpusEncoderConfig::default();
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let mut decoder = OpusDecoder::new(fmt).unwrap();

    let mut output_samples: Vec<f32> = Vec::new();
    for frame in &frames {
        let packet = encoder.encode(frame).unwrap();
        let decoded = decoder.decode(&packet).unwrap();
        output_samples.extend_from_slice(decoded.as_f32_samples());
    }

    let skip = FRAME_SIZE * 5;
    let steady = &output_samples[skip..];

    // THD+N should be below 30% - Opus is lossy but not that noisy.
    let distortion = thd_n(steady, freq);
    assert!(
        distortion < 0.30,
        "THD+N should be < 30%, got {:.1}%",
        distortion * 100.0
    );
}

#[test]
fn opus_roundtrip_level_stability() {
    let freq = 440.0;
    let amplitude = 0.3;
    let frames = sine_frames(freq, amplitude, 100); // 2 seconds

    let config = OpusEncoderConfig::default();
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let mut decoder = OpusDecoder::new(fmt).unwrap();

    let mut frame_rms_values: Vec<f32> = Vec::new();
    for frame in &frames {
        let packet = encoder.encode(frame).unwrap();
        let decoded = decoder.decode(&packet).unwrap();
        frame_rms_values.push(rms(decoded.as_f32_samples()));
    }

    // Skip first 5 frames for warm-up.
    let steady = &frame_rms_values[5..];
    let mean_rms: f32 = steady.iter().sum::<f32>() / steady.len() as f32;
    let max_rms = steady.iter().copied().fold(0.0f32, f32::max);
    let min_rms = steady.iter().copied().fold(f32::MAX, f32::min);

    // Level should be stable: max/min ratio should be < 2.0
    let ratio = if min_rms > 1e-6 {
        max_rms / min_rms
    } else {
        f32::MAX
    };

    println!(
        "Opus roundtrip level: mean={mean_rms:.4}, min={min_rms:.4}, max={max_rms:.4}, ratio={ratio:.2}"
    );
    assert!(
        ratio < 2.0,
        "Level fluctuation ratio {ratio:.2} is too high (max={max_rms:.4}, min={min_rms:.4})"
    );
}

// ────────────────────────────────────────────────────────────────────
//  Tests: Noise gate
// ────────────────────────────────────────────────────────────────────

#[test]
fn noise_gate_does_not_cut_loud_signal() {
    let config = NoiseGateConfig {
        open_threshold: 0.01,
        close_threshold: 0.008,
        hold_frames: 15,
        attack_samples: 48,
        release_samples: 96,
    };
    let mut gate = NoiseGate::new(config);

    let freq = 440.0;
    let amplitude = 0.3; // well above threshold
    let frames = sine_frames(freq, amplitude, 50);

    let mut output_rms: Vec<f32> = Vec::new();
    let mut any_silent = false;
    for mut frame in frames {
        gate.process(&mut frame).unwrap();
        if frame.is_silent {
            any_silent = true;
        }
        output_rms.push(rms(frame.as_f32_samples()));
    }

    // No frame should be marked silent for a continuous loud signal.
    assert!(!any_silent, "Noise gate should not silence a loud signal");

    // Check RMS stability after the first frame (which has fade-in).
    let steady = &output_rms[1..];
    let mean = steady.iter().sum::<f32>() / steady.len() as f32;
    for (i, &r) in steady.iter().enumerate() {
        let deviation = (r - mean).abs() / mean;
        assert!(
            deviation < 0.05,
            "Frame {} RMS {r:.4} deviates {:.1}% from mean {mean:.4}",
            i + 1,
            deviation * 100.0
        );
    }
}

#[test]
fn noise_gate_fade_does_not_corrupt_spectrum() {
    let config = NoiseGateConfig {
        open_threshold: 0.01,
        close_threshold: 0.008,
        hold_frames: 15,
        attack_samples: 48,
        release_samples: 96,
    };
    let mut gate = NoiseGate::new(config);

    let freq = 1000.0;
    let amplitude = 0.3;
    let frames = sine_frames(freq, amplitude, 30);

    let mut all_samples: Vec<f32> = Vec::new();
    for mut frame in frames {
        gate.process(&mut frame).unwrap();
        all_samples.extend_from_slice(frame.as_f32_samples());
    }

    // Skip first frame (fade-in region).
    let skip = FRAME_SIZE;
    let steady = &all_samples[skip..];

    // Dominant frequency should still be 1 kHz.
    let (dom_freq, _) = dominant_frequency(steady);
    assert!(
        (dom_freq - freq).abs() < 10.0,
        "After noise gate, dominant freq should be ~{freq} Hz, got {dom_freq} Hz"
    );

    // THD+N should be very low - noise gate on a loud signal should not distort.
    let distortion = thd_n(steady, freq);
    assert!(
        distortion < 0.05,
        "Noise gate THD+N should be < 5%, got {:.1}%",
        distortion * 100.0
    );
}

// ────────────────────────────────────────────────────────────────────
//  Tests: AGC
// ────────────────────────────────────────────────────────────────────

#[test]
fn agc_does_not_distort_steady_signal() {
    let mut agc = AutomaticGainControl::new(AgcConfig::default());

    let freq = 1000.0;
    let amplitude = 0.3;
    let frames = sine_frames(freq, amplitude, 100);

    let mut all_samples: Vec<f32> = Vec::new();
    let mut frame_rms: Vec<f32> = Vec::new();
    for mut frame in frames {
        agc.process(&mut frame).unwrap();
        frame_rms.push(rms(frame.as_f32_samples()));
        all_samples.extend_from_slice(frame.as_f32_samples());
    }

    // After warm-up, RMS should be reasonably stable.
    let skip_frames = 20; // let AGC settle
    let steady_rms = &frame_rms[skip_frames..];
    let mean_rms: f32 = steady_rms.iter().sum::<f32>() / steady_rms.len() as f32;
    let max_rms = steady_rms.iter().copied().fold(0.0f32, f32::max);
    let min_rms = steady_rms.iter().copied().fold(f32::MAX, f32::min);
    let ratio = if min_rms > 1e-6 { max_rms / min_rms } else { f32::MAX };

    println!(
        "AGC level: mean={mean_rms:.4}, min={min_rms:.4}, max={max_rms:.4}, ratio={ratio:.2}"
    );
    assert!(
        ratio < 1.5,
        "AGC level ratio {ratio:.2} too high (pumping) - mean={mean_rms:.4}"
    );

    // Spectral check: dominant frequency should be unchanged.
    let skip_samples = FRAME_SIZE * skip_frames;
    let steady_samples = &all_samples[skip_samples..];
    let (dom_freq, _) = dominant_frequency(steady_samples);
    assert!(
        (dom_freq - freq).abs() < 10.0,
        "AGC should not shift frequency: expected ~{freq} Hz, got {dom_freq} Hz"
    );

    // THD+N should be low.
    let distortion = thd_n(steady_samples, freq);
    assert!(
        distortion < 0.10,
        "AGC THD+N should be < 10%, got {:.1}%",
        distortion * 100.0
    );
}

#[test]
fn agc_level_stability_across_volume_change() {
    let mut agc = AutomaticGainControl::new(AgcConfig::default());

    let freq = 440.0;
    // Start quiet, then jump to loud.
    let quiet_frames = sine_frames(freq, 0.05, 50);
    let loud_frames = sine_frames(freq, 0.5, 50);

    let mut frame_rms: Vec<f32> = Vec::new();
    for mut frame in quiet_frames.into_iter().chain(loud_frames.into_iter()) {
        agc.process(&mut frame).unwrap();
        frame_rms.push(rms(frame.as_f32_samples()));
    }

    // After settling on each level, values should be near the target.
    // Check the last 20 frames of each segment.
    let quiet_settled = &frame_rms[30..50];
    let loud_settled = &frame_rms[80..100];

    let quiet_mean: f32 = quiet_settled.iter().sum::<f32>() / quiet_settled.len() as f32;
    let loud_mean: f32 = loud_settled.iter().sum::<f32>() / loud_settled.len() as f32;

    println!("AGC volume change: quiet_mean={quiet_mean:.4}, loud_mean={loud_mean:.4}");

    // Both should be somewhat close to the target level (0.25).
    // The quiet section will be limited by max_gain.
    // The important thing: there shouldn't be huge fluctuation within each segment.
    let quiet_max = quiet_settled.iter().copied().fold(0.0f32, f32::max);
    let quiet_min = quiet_settled.iter().copied().fold(f32::MAX, f32::min);
    let quiet_ratio = if quiet_min > 1e-6 { quiet_max / quiet_min } else { f32::MAX };

    let loud_max = loud_settled.iter().copied().fold(0.0f32, f32::max);
    let loud_min = loud_settled.iter().copied().fold(f32::MAX, f32::min);
    let loud_ratio = if loud_min > 1e-6 { loud_max / loud_min } else { f32::MAX };

    assert!(
        quiet_ratio < 1.5,
        "AGC quiet segment ratio {quiet_ratio:.2} too high (pumping)"
    );
    assert!(
        loud_ratio < 1.5,
        "AGC loud segment ratio {loud_ratio:.2} too high (pumping)"
    );
}

// ────────────────────────────────────────────────────────────────────
//  Tests: Full pipeline (noise gate → AGC → Opus encode → decode)
// ────────────────────────────────────────────────────────────────────

#[test]
fn full_pipeline_quality() {
    let freq = 1000.0;
    let amplitude = 0.3;
    let frames = sine_frames(freq, amplitude, 100);

    // Noise gate config (matches what state.rs uses).
    let mut gate = NoiseGate::new(NoiseGateConfig {
        open_threshold: 0.01,
        close_threshold: 0.008,
        hold_frames: 15,
        attack_samples: 48,
        release_samples: 96,
    });

    let mut agc = AutomaticGainControl::new(AgcConfig::default());

    let config = OpusEncoderConfig::default();
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let mut decoder = OpusDecoder::new(fmt).unwrap();

    let mut output_samples: Vec<f32> = Vec::new();
    let mut frame_rms: Vec<f32> = Vec::new();

    for mut frame in frames {
        // Apply filters exactly like the real pipeline.
        gate.process(&mut frame).unwrap();
        agc.process(&mut frame).unwrap();

        // Encode → decode.
        let packet = encoder.encode(&frame).unwrap();
        let decoded = decoder.decode(&packet).unwrap();
        let decoded_samples = decoded.as_f32_samples();
        frame_rms.push(rms(decoded_samples));
        output_samples.extend_from_slice(decoded_samples);
    }

    // Skip warm-up.
    let skip_frames = 10;
    let skip_samples = FRAME_SIZE * skip_frames;
    let steady_samples = &output_samples[skip_samples..];
    let steady_rms = &frame_rms[skip_frames..];

    // 1) Frequency should be preserved.
    let (dom_freq, _) = dominant_frequency(steady_samples);
    let freq_err = (dom_freq - freq).abs();
    println!("Full pipeline: dominant freq = {dom_freq:.1} Hz (expected {freq} Hz, err {freq_err:.1})");
    assert!(freq_err < 15.0, "Frequency shifted too much: {dom_freq} Hz");

    // 2) THD+N should be acceptable.
    let distortion = thd_n(steady_samples, freq);
    println!("Full pipeline: THD+N = {:.1}%", distortion * 100.0);
    assert!(
        distortion < 0.35,
        "Full pipeline THD+N {:.1}% exceeds 35%",
        distortion * 100.0
    );

    // 3) Level stability: should not have pumping > 3:1.
    let mean_rms: f32 = steady_rms.iter().sum::<f32>() / steady_rms.len() as f32;
    let max_rms = steady_rms.iter().copied().fold(0.0f32, f32::max);
    let min_rms = steady_rms.iter().copied().fold(f32::MAX, f32::min);
    let ratio = if min_rms > 1e-6 { max_rms / min_rms } else { f32::MAX };
    println!(
        "Full pipeline level: mean={mean_rms:.4}, min={min_rms:.4}, max={max_rms:.4}, ratio={ratio:.2}"
    );
    assert!(
        ratio < 3.0,
        "Level fluctuation {ratio:.2} is too high (pumping/cut-off)"
    );

    // 4) No frames should be completely silent (signal is above gate threshold).
    let silent_count = steady_rms.iter().filter(|&&r| r < 0.001).count();
    assert!(
        silent_count == 0,
        "{silent_count} frames were silent - signal is being cut off!"
    );
}
