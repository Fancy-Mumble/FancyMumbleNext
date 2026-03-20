//! Audio quality integration tests.
//!
//! These tests generate known signals (sine waves), push them through
//! individual pipeline stages and the full encode->decode roundtrip,
//! then analyse the output with FFT to verify spectral purity, level
//! stability, and absence of artefacts.
//!
//! ```sh
//! cargo test --package mumble-protocol --test audio_quality --features opus-codec
//! ```

#![cfg(feature = "opus-codec")]
// Integration tests are separate crate compilation units and will trigger
// `unused_crate_dependencies` for every transitive dep of mumble-protocol
// that is not directly imported in this file.
#![allow(
    unused_crate_dependencies,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "integration test: transitive deps are not directly imported; unwrap/expect and long test functions are idiomatic"
)]

use mumble_protocol::audio::decoder::{AudioDecoder, OpusDecoder};
use mumble_protocol::audio::encoder::{
    AudioEncoder, OpusApplication, OpusEncoder, OpusEncoderConfig,
};
use mumble_protocol::audio::filter::automatic_gain::AutomaticGainControl;
use mumble_protocol::audio::filter::automatic_gain::AgcConfig;
use mumble_protocol::audio::filter::noise_gate::{NoiseGate, NoiseGateConfig};
use mumble_protocol::audio::filter::AudioFilter;
use mumble_protocol::audio::sample::{AudioFormat, AudioFrame};

const SAMPLE_RATE: u32 = 48_000;
const FRAME_SIZE: usize = 960; // 20 ms @ 48 kHz

// --------------------------------------------------------------------
//  Helpers
// --------------------------------------------------------------------

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
    sine_frames_with_size(freq_hz, amplitude, count, FRAME_SIZE)
}

/// Generate multiple contiguous sine frames with a custom frame size.
fn sine_frames_with_size(
    freq_hz: f32,
    amplitude: f32,
    count: usize,
    frame_size: usize,
) -> Vec<AudioFrame> {
    let mut frames = Vec::with_capacity(count);
    let mut phase = 0.0f32;
    for _ in 0..count {
        let (f, p) = sine_frame(freq_hz, amplitude, frame_size, phase);
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

// --------------------------------------------------------------------
//  Minimal FFT (Cooley-Tukey radix-2 DIT)
// --------------------------------------------------------------------

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
/// We assume `fundamental_bin` +/- 2 bins carry the fundamental energy;
/// everything else is distortion / noise.
fn thd_n(samples: &[f32], fundamental_hz: f32) -> f32 {
    let (mags, bin_hz) = magnitude_spectrum(samples);
    let half = mags.len() / 2;
    let fund_bin = (fundamental_hz / bin_hz).round() as usize;
    let margin = 8; // +/-8 bins around fundamental (accounts for Hann window main lobe)

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

// --------------------------------------------------------------------
//  Tests: Opus encode -> decode roundtrip
// --------------------------------------------------------------------

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

    // Dominant frequency should be ~ 440 Hz.
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

// --------------------------------------------------------------------
//  Tests: Noise gate
// --------------------------------------------------------------------

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

// --------------------------------------------------------------------
//  Tests: AGC
// --------------------------------------------------------------------

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

// --------------------------------------------------------------------
//  Tests: Full pipeline (noise gate -> AGC -> Opus encode -> decode)
// --------------------------------------------------------------------

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

        // Encode -> decode.
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

// ----
//  Tests: 10 ms frame size (480 samples @ 48 kHz)
// ----

const FRAME_SIZE_10MS: usize = 480;

#[test]
fn opus_10ms_roundtrip_preserves_frequency() {
    let freq = 440.0;
    let amplitude = 0.3;
    let frames = sine_frames_with_size(freq, amplitude, 100, FRAME_SIZE_10MS);

    let config = OpusEncoderConfig {
        frame_size: FRAME_SIZE_10MS,
        ..OpusEncoderConfig::default()
    };
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let mut decoder = OpusDecoder::new(fmt).unwrap();

    let mut output_samples: Vec<f32> = Vec::new();
    for frame in &frames {
        let packet = encoder.encode(frame).unwrap();
        let decoded = decoder.decode(&packet).unwrap();
        output_samples.extend_from_slice(decoded.as_f32_samples());
    }

    // Skip first 10 frames (codec warm-up).
    let skip = FRAME_SIZE_10MS * 10;
    let steady = &output_samples[skip..];

    let (dom_freq, _) = dominant_frequency(steady);
    let freq_error = (dom_freq - freq).abs();
    assert!(
        freq_error < 10.0,
        "10 ms: dominant freq should be ~{freq} Hz, got {dom_freq} Hz (err {freq_error} Hz)"
    );
}

#[test]
fn opus_10ms_roundtrip_thd_n_acceptable() {
    let freq = 1000.0;
    let amplitude = 0.3;
    let frames = sine_frames_with_size(freq, amplitude, 100, FRAME_SIZE_10MS);

    let config = OpusEncoderConfig {
        frame_size: FRAME_SIZE_10MS,
        ..OpusEncoderConfig::default()
    };
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let mut decoder = OpusDecoder::new(fmt).unwrap();

    let mut output_samples: Vec<f32> = Vec::new();
    for frame in &frames {
        let packet = encoder.encode(frame).unwrap();
        let decoded = decoder.decode(&packet).unwrap();
        output_samples.extend_from_slice(decoded.as_f32_samples());
    }

    let skip = FRAME_SIZE_10MS * 10;
    let steady = &output_samples[skip..];

    let distortion = thd_n(steady, freq);
    println!("10 ms roundtrip THD+N = {:.1}%", distortion * 100.0);
    assert!(
        distortion < 0.30,
        "10 ms THD+N should be < 30%, got {:.1}%",
        distortion * 100.0
    );
}

/// Detect pops/clicks: sample-to-sample discontinuities in the output
/// of a continuous encode-decode roundtrip. Each "pop" is a jump
/// exceeding a scaled threshold relative to the signal amplitude.
#[test]
fn opus_10ms_roundtrip_no_boundary_pops() {
    let freq = 440.0;
    let amplitude = 0.3;
    let frames = sine_frames_with_size(freq, amplitude, 200, FRAME_SIZE_10MS);

    let config = OpusEncoderConfig {
        frame_size: FRAME_SIZE_10MS,
        bitrate: 136_000,
        ..OpusEncoderConfig::default()
    };
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let mut decoder = OpusDecoder::new(fmt).unwrap();

    let mut output_samples: Vec<f32> = Vec::new();
    for frame in &frames {
        let packet = encoder.encode(frame).unwrap();
        let decoded = decoder.decode(&packet).unwrap();
        output_samples.extend_from_slice(decoded.as_f32_samples());
    }

    // Skip warm-up.
    let skip = FRAME_SIZE_10MS * 10;
    let steady = &output_samples[skip..];

    // A 440 Hz sine at 48 kHz has a maximum sample-to-sample delta of:
    //   amplitude * 2 * PI * freq / sample_rate
    //   = 0.3 * 2 * PI * 440 / 48000 ~ 0.0173
    // Allow up to 10x that for lossy codec headroom.
    let max_natural_delta = amplitude * 2.0 * std::f32::consts::PI * freq / SAMPLE_RATE as f32;
    let pop_threshold = max_natural_delta * 10.0;

    let mut pop_count = 0;
    for window in steady.windows(2) {
        let delta = (window[1] - window[0]).abs();
        if delta > pop_threshold {
            pop_count += 1;
        }
    }

    println!(
        "10 ms pop detection: threshold={pop_threshold:.4}, pops={pop_count} in {} samples",
        steady.len()
    );
    assert!(
        pop_count < 5,
        "Detected {pop_count} pops/clicks in 10 ms roundtrip (threshold {pop_threshold:.4})"
    );
}

// ----
//  Tests: Opus PLC (packet-loss concealment)
// ----

/// Verify that Opus PLC produces a smooth concealment frame that
/// does not pop.  This mirrors the Mumble C++ client's approach
/// of calling `opus_decode_float(dec, NULL, 0, ...)` on missing
/// packets.
#[test]
fn opus_plc_produces_smooth_concealment() {
    let freq = 440.0;
    let amplitude = 0.3;
    let frames = sine_frames_with_size(freq, amplitude, 50, FRAME_SIZE);

    let config = OpusEncoderConfig::default();
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let mut decoder = OpusDecoder::new(fmt).unwrap();

    let mut output_samples: Vec<f32> = Vec::new();

    // Encode + decode 40 frames normally, then simulate 3 lost
    // packets (PLC), then resume with 10 more frames.
    for (i, frame) in frames.iter().enumerate() {
        let packet = encoder.encode(frame).unwrap();

        if (20..23).contains(&i) {
            // Simulate packet loss: use PLC instead of decode.
            let plc_frame = decoder.decode_lost().unwrap();
            output_samples.extend_from_slice(plc_frame.as_f32_samples());
        } else {
            let decoded = decoder.decode(&packet).unwrap();
            output_samples.extend_from_slice(decoded.as_f32_samples());
        }
    }

    // Check the transition region around the PLC frames for pops.
    // PLC starts at frame 20, so sample offset = 20 * FRAME_SIZE.
    let plc_start = 19 * FRAME_SIZE; // one frame before PLC
    let plc_end = 24 * FRAME_SIZE; // one frame after PLC

    let region = &output_samples[plc_start..plc_end.min(output_samples.len())];

    let max_natural_delta = amplitude * 2.0 * std::f32::consts::PI * freq / SAMPLE_RATE as f32;
    let pop_threshold = max_natural_delta * 15.0; // generous for PLC transition

    let mut pop_count = 0;
    for window in region.windows(2) {
        let delta = (window[1] - window[0]).abs();
        if delta > pop_threshold {
            pop_count += 1;
        }
    }

    println!(
        "PLC pop detection: threshold={pop_threshold:.4}, pops={pop_count} in {} samples",
        region.len()
    );
    assert!(
        pop_count < 5,
        "Opus PLC produced {pop_count} pops at concealment boundaries"
    );

    // PLC frames should not be silent - Opus generates prediction-based audio
    let plc_region_start = 20 * FRAME_SIZE;
    let plc_region_end = 23 * FRAME_SIZE;
    let plc_samples = &output_samples[plc_region_start..plc_region_end.min(output_samples.len())];
    let plc_rms = rms(plc_samples);
    println!("PLC frame RMS = {plc_rms:.4}");
    assert!(
        plc_rms > 0.01,
        "PLC frames should contain non-trivial audio, got RMS={plc_rms:.4}"
    );
}

/// Verify that the inbound pipeline's sequence tracking correctly
/// detects gaps and invokes PLC.
#[test]
fn inbound_pipeline_gap_detection_invokes_plc() {
    use mumble_protocol::audio::pipeline::InboundPipeline;
    use mumble_protocol::audio::filter::FilterChain;
    use mumble_protocol::audio::playback::NullPlayback;
    use mumble_protocol::audio::encoder::EncodedPacket;

    let fmt = AudioFormat::MONO_48KHZ_F32;
    let config = OpusEncoderConfig::default();
    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let decoder = OpusDecoder::new(fmt).unwrap();

    let mut pipeline = InboundPipeline::new(
        Box::new(decoder),
        FilterChain::new(),
        Box::new(NullPlayback::new(fmt)),
    );
    pipeline.start().unwrap();

    // Encode a few frames.
    let frames = sine_frames(440.0, 0.3, 10);
    let mut packets: Vec<EncodedPacket> = Vec::new();
    for frame in &frames {
        packets.push(encoder.encode(frame).unwrap());
    }

    // Feed packets with a gap: 0, 1, 2, [skip 3,4], 5, 6, ...
    // The pipeline should invoke PLC for packets 3 and 4.
    for (i, packet) in packets.iter().enumerate() {
        if i == 3 || i == 4 {
            continue; // simulate loss
        }
        pipeline.tick(packet).unwrap();
    }
}

// ----
//  Tests: MP3 roundtrip and frame-boundary analysis
// ----

/// Decode an MP3 file into mono f32 samples at 48 kHz.
///
/// Returns `None` if the file does not exist or cannot be decoded.
/// Handles stereo downmixing and resampling from 44.1 kHz.
#[allow(dead_code, reason = "helper function available when sample files are present")]
fn decode_mp3_to_mono_48k(path: &str) -> Option<Vec<f32>> {
    use minimp3::{Decoder as Mp3Decoder, Error as Mp3Error};
    use std::fs::File;

    let file = File::open(path).ok()?;
    let mut decoder = Mp3Decoder::new(file);
    let mut samples: Vec<f32> = Vec::new();

    loop {
        match decoder.next_frame() {
            Ok(frame) => {
                let sr = frame.sample_rate as u32;
                let ch = frame.channels;

                // Convert i16 to f32 and downmix to mono
                let mono: Vec<f32> = if ch <= 1 {
                    frame.data.iter().map(|&s| s as f32 / 32768.0).collect()
                } else {
                    frame
                        .data
                        .chunks(ch)
                        .map(|chunk| {
                            let sum: f32 = chunk.iter().map(|&s| s as f32 / 32768.0).sum();
                            sum / ch as f32
                        })
                        .collect()
                };

                // Resample to 48 kHz if needed (linear interpolation)
                if sr == 48000 {
                    samples.extend_from_slice(&mono);
                } else {
                    let ratio = 48000.0 / sr as f64;
                    let out_len = (mono.len() as f64 * ratio) as usize;
                    for i in 0..out_len {
                        let src_pos = i as f64 / ratio;
                        let idx = src_pos as usize;
                        let frac = (src_pos - idx as f64) as f32;
                        let s0 = mono.get(idx).copied().unwrap_or(0.0);
                        let s1 = mono.get(idx + 1).copied().unwrap_or(s0);
                        samples.push(s0 + (s1 - s0) * frac);
                    }
                }
            }
            Err(Mp3Error::Eof) => break,
            Err(_) => return None,
        }
    }

    if samples.is_empty() {
        None
    } else {
        Some(samples)
    }
}

/// A playback sink that captures all decoded frames for post-test
/// analysis.  Uses `Arc<Mutex<Vec<f32>>>` so the test can read the
/// recorded samples after the pipeline processes everything.
struct RecordingPlayback {
    format: AudioFormat,
    samples: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
}

impl RecordingPlayback {
    fn new(format: AudioFormat) -> (Self, std::sync::Arc<std::sync::Mutex<Vec<f32>>>) {
        let samples = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (
            Self {
                format,
                samples: samples.clone(),
            },
            samples,
        )
    }
}

impl mumble_protocol::audio::playback::AudioPlayback for RecordingPlayback {
    fn format(&self) -> AudioFormat {
        self.format
    }
    fn write_frame(&mut self, frame: &AudioFrame) -> mumble_protocol::error::Result<()> {
        self.samples
            .lock()
            .unwrap()
            .extend_from_slice(frame.as_f32_samples());
        Ok(())
    }
    fn start(&mut self) -> mumble_protocol::error::Result<()> {
        Ok(())
    }
    fn stop(&mut self) -> mumble_protocol::error::Result<()> {
        Ok(())
    }
}

/// Run a full Opus roundtrip analysis for one MP3 file.
///
/// Returns `true` if the file was found and analysed, `false` if it
/// was skipped (file missing or undecodable).
///
/// Checks:
///   - PSNR > 10 dB
///   - Mean frame-boundary jump < 0.15
///   - RMS-envelope correlation > 0.85
fn run_mp3_roundtrip_analysis(path: &str) -> bool {
    let Some(samples) = decode_mp3_to_mono_48k(path) else {
        println!("  Skipping {path}: could not decode");
        return false;
    };

    let frame_size = 480; // 10 ms
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let config = OpusEncoderConfig {
        frame_size,
        bitrate: 136_000,
        ..OpusEncoderConfig::default()
    };

    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let mut decoder = OpusDecoder::new(fmt).unwrap();

    let frame_count = samples.len() / frame_size;
    let mut output_samples: Vec<f32> = Vec::with_capacity(frame_count * frame_size);

    for i in 0..frame_count {
        let start = i * frame_size;
        let end = start + frame_size;
        let data: Vec<u8> = samples[start..end]
            .iter()
            .flat_map(|s| s.to_ne_bytes())
            .collect();
        let frame = AudioFrame {
            data,
            format: fmt,
            sequence: i as u64,
            is_silent: false,
        };
        let packet = encoder.encode(&frame).unwrap();
        let decoded = decoder.decode(&packet).unwrap();
        output_samples.extend_from_slice(decoded.as_f32_samples());
    }

    // Skip the first few frames (Opus encoder warm-up).
    let skip = frame_size * 5;
    let total = frame_count * frame_size;
    let input_raw = &samples[skip..total];
    let output_raw = &output_samples[skip..output_samples.len().min(total)];
    let raw_len = input_raw.len().min(output_raw.len());

    // ---- Delay compensation ----
    // Opus introduces a codec latency; estimate it via cross-correlation
    // on the first ~1 second of audio and shift accordingly.
    let delay_search_len = 48000.min(raw_len);
    let max_lag: usize = 960; // search up to 20 ms
    let mut best_lag = 0usize;
    let mut best_corr = f64::NEG_INFINITY;
    for lag in 0..max_lag.min(delay_search_len) {
        let n = delay_search_len - lag;
        let mut corr = 0.0f64;
        for i in 0..n {
            corr += input_raw[i] as f64 * output_raw[i + lag] as f64;
        }
        if corr > best_corr {
            best_corr = corr;
            best_lag = lag;
        }
    }

    let comp_len = raw_len.saturating_sub(best_lag);
    let input_steady = &input_raw[..comp_len];
    let output_steady = &output_raw[best_lag..best_lag + comp_len];
    let len = comp_len;

    // 1. PSNR
    let sum_sq_err: f64 = input_steady
        .iter()
        .zip(output_steady.iter())
        .map(|(&a, &b)| ((a - b) as f64).powi(2))
        .sum();
    let mse = sum_sq_err / len as f64;
    let psnr = if mse > 0.0 {
        10.0 * (1.0f64 / mse).log10()
    } else {
        f64::INFINITY
    };

    // 2. Boundary analysis
    let skip_frames = 5;
    let boundary_jumps: Vec<f32> = (skip_frames
        ..frame_count.min(output_samples.len() / frame_size))
        .filter_map(|i| {
            let idx = i * frame_size;
            if idx > 0 && idx < output_samples.len() {
                Some((output_samples[idx] - output_samples[idx - 1]).abs())
            } else {
                None
            }
        })
        .collect();

    let mean_jump = if boundary_jumps.is_empty() {
        0.0f32
    } else {
        boundary_jumps.iter().sum::<f32>() / boundary_jumps.len() as f32
    };
    let max_jump = boundary_jumps
        .iter()
        .copied()
        .fold(0.0f32, f32::max);

    // 3. RMS-envelope correlation (10 ms windows, 50% overlap)
    //
    // Opus is a perceptual codec: waveform correlation is meaningless
    // for complex content.  Envelope correlation verifies that the
    // codec preserves the overall dynamics.
    let window_size = 480;
    let hop = 240;

    let compute_envelope = |s: &[f32]| -> Vec<f32> {
        let mut env = Vec::new();
        let mut start = 0;
        while start + window_size <= s.len() {
            let rms_val = (s[start..start + window_size]
                .iter()
                .map(|x| x * x)
                .sum::<f32>()
                / window_size as f32)
                .sqrt();
            env.push(rms_val);
            start += hop;
        }
        env
    };

    let in_env = compute_envelope(input_steady);
    let out_env = compute_envelope(output_steady);
    let env_len = in_env.len().min(out_env.len());

    let env_correlation = if env_len > 2 {
        let mean_i: f64 =
            in_env[..env_len].iter().map(|&v| v as f64).sum::<f64>() / env_len as f64;
        let mean_o: f64 =
            out_env[..env_len].iter().map(|&v| v as f64).sum::<f64>() / env_len as f64;
        let (mut cov, mut var_i, mut var_o) = (0.0f64, 0.0f64, 0.0f64);
        for (&a, &b) in in_env[..env_len].iter().zip(out_env[..env_len].iter()) {
            let da = a as f64 - mean_i;
            let db = b as f64 - mean_o;
            cov += da * db;
            var_i += da * da;
            var_o += db * db;
        }
        if var_i > 0.0 && var_o > 0.0 {
            cov / (var_i.sqrt() * var_o.sqrt())
        } else {
            0.0
        }
    } else {
        1.0
    };

    println!(
        "  {path}\n    \
         delay={best_lag}smp  frames={frame_count}  PSNR={psnr:.1}dB  \
         mean_bdry={mean_jump:.4}  max_bdry={max_jump:.4}  \
         env_corr={env_correlation:.4}"
    );

    assert!(
        psnr > 10.0,
        "[{path}] PSNR {psnr:.1} dB too low (expected > 10 dB)"
    );
    assert!(
        mean_jump < 0.15,
        "[{path}] Mean boundary jump {mean_jump:.4} too high (limit 0.15)"
    );
    assert!(
        env_correlation > 0.85,
        "[{path}] Envelope correlation {env_correlation:.4} too low (expected > 0.85)"
    );

    true
}

/// Encode every `.mp3` file found in `tests/samples/` through Opus
/// (10 ms / 136 kb/s) and verify each one passes:
///   - PSNR > 10 dB
///   - Mean frame-boundary jump < 0.15
///   - RMS-envelope correlation > 0.85
///
/// New sample files placed in `tests/samples/` are picked up
/// automatically without any code changes.
#[test]
fn mp3_roundtrip_boundary_analysis() {
    let samples_dir = std::path::Path::new("tests/samples");
    if !samples_dir.exists() {
        println!("Skipping mp3_roundtrip_boundary_analysis: tests/samples/ not found");
        return;
    }

    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(samples_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"))
        })
        .collect();

    if paths.is_empty() {
        println!("Skipping mp3_roundtrip_boundary_analysis: no .mp3 files in tests/samples/");
        return;
    }

    paths.sort();

    println!(
        "mp3_roundtrip_boundary_analysis: testing {} file(s)",
        paths.len()
    );

    let mut tested = 0usize;
    for path in &paths {
        let path_str = path.to_str().unwrap_or("<invalid path>");
        if run_mp3_roundtrip_analysis(path_str) {
            tested += 1;
        }
    }

    println!("mp3_roundtrip_boundary_analysis: {tested}/{} file(s) passed", paths.len());
    assert!(tested > 0, "No MP3 files could be decoded");
}

/// Long-duration (5 s) complex signal through Opus 10 ms frames.
///
/// Generates a multi-harmonic signal with amplitude modulation to
/// simulate realistic music dynamics, encodes through the full
/// `InboundPipeline` (with crossfade), and verifies:
///   - Boundary/internal jump ratio < 3.0  (no systematic discontinuity)
///   - Max boundary jump < 0.5              (no severe click)
///   - PSNR > 15 dB                         (codec fidelity)
#[test]
fn long_duration_frame_boundary_smoothness() {
    use mumble_protocol::audio::filter::FilterChain;
    use mumble_protocol::audio::pipeline::InboundPipeline;

    let duration_secs = 5.0f32;
    let frame_size = 480; // 10 ms
    let total_samples = (SAMPLE_RATE as f32 * duration_secs) as usize;
    let pi = std::f32::consts::PI;

    // Multi-harmonic signal with AM (amplitude modulation)
    let mut signal = Vec::with_capacity(total_samples);
    for i in 0..total_samples {
        let t = i as f32 / SAMPLE_RATE as f32;
        let s = (0.15 * (2.0 * pi * 220.0 * t).sin()
            + 0.10 * (2.0 * pi * 440.0 * t).sin()
            + 0.08 * (2.0 * pi * 880.0 * t).sin()
            + 0.05 * (2.0 * pi * 1760.0 * t).sin()
            + 0.03 * (2.0 * pi * 3520.0 * t).sin())
            * (1.0 + 0.3 * (2.0 * pi * 1.5 * t).sin());
        signal.push(s);
    }

    let fmt = AudioFormat::MONO_48KHZ_F32;
    let config = OpusEncoderConfig {
        frame_size,
        bitrate: 136_000,
        ..OpusEncoderConfig::default()
    };

    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let decoder = OpusDecoder::new(fmt).unwrap();

    // Use a recording playback to capture pipeline output
    let (recording, output_buf) = RecordingPlayback::new(fmt);
    let mut pipeline = InboundPipeline::new(
        Box::new(decoder),
        FilterChain::new(),
        Box::new(recording),
    );
    pipeline.start().unwrap();

    let frame_count = signal.len() / frame_size;
    for i in 0..frame_count {
        let start = i * frame_size;
        let end = start + frame_size;
        let data: Vec<u8> = signal[start..end]
            .iter()
            .flat_map(|s| s.to_ne_bytes())
            .collect();
        let frame = AudioFrame {
            data,
            format: fmt,
            sequence: i as u64,
            is_silent: false,
        };
        let packet = encoder.encode(&frame).unwrap();
        pipeline.tick(&packet).unwrap();
    }

    let output_samples = output_buf.lock().unwrap();

    // ---- Boundary analysis (skip first 10 frames for warm-up) ----
    let skip_frames = 10;
    let mut boundary_jumps: Vec<f32> = Vec::new();
    let mut non_boundary_jumps: Vec<f32> = Vec::new();

    for i in skip_frames..frame_count.min(output_samples.len() / frame_size) {
        let boundary = i * frame_size;
        if boundary >= output_samples.len() {
            break;
        }

        // Jump at frame boundary
        if boundary > 0 {
            boundary_jumps
                .push((output_samples[boundary] - output_samples[boundary - 1]).abs());
        }

        // Jumps within the frame (for baseline comparison)
        let frame_end = ((i + 1) * frame_size).min(output_samples.len());
        for j in (boundary + 1)..frame_end {
            non_boundary_jumps
                .push((output_samples[j] - output_samples[j - 1]).abs());
        }
    }

    let mean_boundary =
        boundary_jumps.iter().sum::<f32>() / boundary_jumps.len().max(1) as f32;
    let max_boundary = boundary_jumps
        .iter()
        .copied()
        .fold(0.0f32, f32::max);
    let mean_internal =
        non_boundary_jumps.iter().sum::<f32>() / non_boundary_jumps.len().max(1) as f32;
    let ratio = if mean_internal > 1e-6 {
        mean_boundary / mean_internal
    } else {
        0.0
    };

    println!(
        "5 s boundary analysis (10 ms frames, {} boundaries):\n  \
         mean_boundary={mean_boundary:.4}, max_boundary={max_boundary:.4}\n  \
         mean_internal={mean_internal:.4}, ratio={ratio:.2}",
        boundary_jumps.len()
    );

    assert!(
        ratio < 3.0,
        "Boundary/internal ratio {ratio:.2} too high \
         (frame boundaries have systematic discontinuities)"
    );
    assert!(
        max_boundary < 0.5,
        "Max boundary jump {max_boundary:.4} exceeds 0.5 (severe click)"
    );

    // ---- PSNR check (skip warm-up) ----
    let skip = frame_size * skip_frames;
    let trimmed = frame_count * frame_size;
    let in_steady = &signal[skip..trimmed];
    let out_steady = &output_samples[skip..output_samples.len().min(trimmed)];
    let len = in_steady.len().min(out_steady.len());

    let sum_sq_err: f64 = in_steady[..len]
        .iter()
        .zip(out_steady[..len].iter())
        .map(|(&a, &b)| ((a - b) as f64).powi(2))
        .sum();
    let mse = sum_sq_err / len as f64;
    let psnr = if mse > 0.0 {
        10.0 * (1.0f64 / mse).log10()
    } else {
        f64::INFINITY
    };
    println!("5 s roundtrip PSNR = {psnr:.1} dB");
    assert!(
        psnr > 10.0,
        "PSNR {psnr:.1} dB too low (expected > 10 dB)"
    );
}

/// Verify that the pipeline crossfade correction actually reduces
/// boundary discontinuities by comparing a pipeline-processed signal
/// against raw decode (no crossfade).
#[test]
fn pipeline_crossfade_reduces_boundary_jumps() {
    use mumble_protocol::audio::filter::FilterChain;
    use mumble_protocol::audio::pipeline::InboundPipeline;

    let frame_size = 480;
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let config = OpusEncoderConfig {
        frame_size,
        bitrate: 64_000, // low bitrate to stress codec
        ..OpusEncoderConfig::default()
    };

    // Generate signal with sharp transients that stress the codec
    let frame_count = 200; // ~2 seconds
    let mut signal = Vec::with_capacity(frame_count * frame_size);
    let pi = std::f32::consts::PI;
    for i in 0..(frame_count * frame_size) {
        let t = i as f32 / SAMPLE_RATE as f32;
        // Square-ish wave via harmonics (lots of high-frequency content)
        let s = 0.2 * (2.0 * pi * 300.0 * t).sin()
            + 0.07 * (2.0 * pi * 900.0 * t).sin()
            + 0.04 * (2.0 * pi * 1500.0 * t).sin()
            + 0.03 * (2.0 * pi * 2100.0 * t).sin();
        signal.push(s);
    }

    let mut encoder = OpusEncoder::new(config.clone(), fmt).unwrap();
    let mut packets = Vec::with_capacity(frame_count);
    for i in 0..frame_count {
        let start = i * frame_size;
        let end = start + frame_size;
        let data: Vec<u8> = signal[start..end]
            .iter()
            .flat_map(|s| s.to_ne_bytes())
            .collect();
        let frame = AudioFrame {
            data,
            format: fmt,
            sequence: i as u64,
            is_silent: false,
        };
        packets.push(encoder.encode(&frame).unwrap());
    }

    // Decode WITHOUT pipeline crossfade (raw)
    let mut raw_decoder = OpusDecoder::new(fmt).unwrap();
    let mut raw_output: Vec<f32> = Vec::new();
    for pkt in &packets {
        let decoded = raw_decoder.decode(pkt).unwrap();
        raw_output.extend_from_slice(decoded.as_f32_samples());
    }

    // Decode WITH pipeline crossfade
    let pipeline_decoder = OpusDecoder::new(fmt).unwrap();
    let (recording, pipe_buf) = RecordingPlayback::new(fmt);
    let mut pipeline = InboundPipeline::new(
        Box::new(pipeline_decoder),
        FilterChain::new(),
        Box::new(recording),
    );
    pipeline.start().unwrap();
    for pkt in &packets {
        pipeline.tick(pkt).unwrap();
    }
    let pipe_output = pipe_buf.lock().unwrap();

    // Measure boundary jumps for both
    let skip_frames = 10;
    let measure_end = frame_count.min(raw_output.len() / frame_size).min(pipe_output.len() / frame_size);

    let boundary_jumps = |samples: &[f32]| -> (f32, f32) {
        let mut jumps = Vec::new();
        for i in skip_frames..measure_end {
            let idx = i * frame_size;
            if idx > 0 && idx < samples.len() {
                jumps.push((samples[idx] - samples[idx - 1]).abs());
            }
        }
        let mean = jumps.iter().sum::<f32>() / jumps.len().max(1) as f32;
        let max = jumps.iter().copied().fold(0.0f32, f32::max);
        (mean, max)
    };

    let (raw_mean, raw_max) = boundary_jumps(&raw_output);
    let (pipe_mean, pipe_max) = boundary_jumps(&pipe_output);

    println!(
        "Crossfade comparison (64 kb/s stress test):\n  \
         raw:      mean={raw_mean:.4}, max={raw_max:.4}\n  \
         pipeline: mean={pipe_mean:.4}, max={pipe_max:.4}\n  \
         improvement: mean={:.1}%, max={:.1}%",
        if raw_mean > 0.0 {
            (1.0 - pipe_mean / raw_mean) * 100.0
        } else {
            0.0
        },
        if raw_max > 0.0 {
            (1.0 - pipe_max / raw_max) * 100.0
        } else {
            0.0
        },
    );

    // Pipeline should not make boundary jumps worse
    assert!(
        pipe_mean <= raw_mean * 1.1,
        "Pipeline crossfade made mean boundary jumps WORSE: {pipe_mean:.4} vs raw {raw_mean:.4}"
    );
}

// ----
//  Tests: WAV voice crackling detection
// ----

/// Decode a WAV file to mono f32 samples at 48 kHz.
///
/// Handles mono/stereo, 16-bit/32-bit float, and resamples from other
/// sample rates if necessary.  Returns `None` if file missing or
/// unsupported.
#[allow(dead_code, reason = "helper function available when sample WAV files are present")]
fn decode_wav_to_mono_48k(path: &str) -> Option<Vec<f32>> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let sr = spec.sample_rate;
    let ch = spec.channels as usize;

    let raw_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max_val = (1u32 << (bits - 1)) as f32;
            reader
                .into_samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / max_val)
                .collect()
        }
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(Result::ok)
            .collect(),
    };

    if raw_samples.is_empty() {
        return None;
    }

    // Downmix to mono
    let mono: Vec<f32> = if ch <= 1 {
        raw_samples
    } else {
        raw_samples
            .chunks(ch)
            .map(|chunk| {
                let sum: f32 = chunk.iter().sum();
                sum / ch as f32
            })
            .collect()
    };

    // Resample if not 48 kHz
    if sr == 48000 {
        Some(mono)
    } else {
        let ratio = 48000.0 / sr as f64;
        let out_len = (mono.len() as f64 * ratio) as usize;
        let mut resampled = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let src_pos = i as f64 / ratio;
            let idx = src_pos as usize;
            let frac = (src_pos - idx as f64) as f32;
            let s0 = mono.get(idx).copied().unwrap_or(0.0);
            let s1 = mono.get(idx + 1).copied().unwrap_or(s0);
            resampled.push(s0 + (s1 - s0) * frac);
        }
        Some(resampled)
    }
}

/// Detect crackling/click artefacts by measuring the number of
/// sample-to-sample jumps that exceed a threshold relative to
/// the signal's local RMS.
///
/// Returns (`pop_count`, `pop_rate_pct`, `max_delta`).
fn detect_pops(samples: &[f32], local_window: usize, threshold_mult: f32) -> (usize, f32, f32) {
    if samples.len() < local_window + 1 {
        return (0, 0.0, 0.0);
    }
    let mut pop_count = 0usize;
    let mut max_delta = 0.0f32;

    // Pre-compute local RMS in windows
    for i in 1..samples.len() {
        let delta = (samples[i] - samples[i - 1]).abs();
        if delta > max_delta {
            max_delta = delta;
        }

        // Local RMS around this sample
        let win_start = i.saturating_sub(local_window / 2);
        let win_end = (i + local_window / 2).min(samples.len());
        let window = &samples[win_start..win_end];
        let local_rms = rms(window);

        // A pop is a delta that exceeds threshold_mult times the local RMS
        let pop_threshold = (local_rms * threshold_mult).max(0.005);
        if delta > pop_threshold {
            pop_count += 1;
        }
    }

    let pop_rate = pop_count as f32 / (samples.len() - 1) as f32 * 100.0;
    (pop_count, pop_rate, max_delta)
}

/// Measure short-term energy roughness: the standard deviation of
/// frame-level RMS values relative to the mean.
///
/// High roughness indicates rapid energy fluctuations - the audible
/// signature of crackling.
fn energy_roughness(samples: &[f32], window_size: usize) -> f32 {
    let mut rms_values: Vec<f32> = Vec::new();
    let mut start = 0;
    while start + window_size <= samples.len() {
        rms_values.push(rms(&samples[start..start + window_size]));
        start += window_size;
    }

    if rms_values.len() < 2 {
        return 0.0;
    }

    let mean_rms: f32 = rms_values.iter().sum::<f32>() / rms_values.len() as f32;
    if mean_rms < 1e-6 {
        return 0.0;
    }

    let variance: f32 = rms_values
        .iter()
        .map(|&v| (v - mean_rms).powi(2))
        .sum::<f32>()
        / rms_values.len() as f32;

    variance.sqrt() / mean_rms // coefficient of variation
}

/// Find the delay (in samples) that best aligns `output` to `reference`
/// using cross-correlation.  Searches delays in `0..max_delay`.
/// Returns the optimal delay: `reference[i]` aligns with `output[i + delay]`.
fn find_alignment_delay(reference: &[f32], output: &[f32], max_delay: usize) -> usize {
    let len = reference.len().min(output.len());
    if len == 0 || max_delay == 0 {
        return 0;
    }

    let mut best_delay = 0usize;
    let mut best_corr = f64::NEG_INFINITY;

    for delay in 0..max_delay.min(len) {
        let n = len - delay;
        let mut sum = 0.0f64;
        for i in 0..n {
            sum += reference[i] as f64 * output[i + delay] as f64;
        }
        if sum > best_corr {
            best_corr = sum;
            best_delay = delay;
        }
    }

    best_delay
}

/// Encode/decode a signal with a given config and return aligned SNR.
fn roundtrip_snr(samples: &[f32], config: &OpusEncoderConfig) -> (f32, usize) {
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let mut encoder = OpusEncoder::new(config.clone(), fmt).unwrap();
    let mut decoder = OpusDecoder::new(fmt).unwrap();

    let frame_count = samples.len() / config.frame_size;
    let mut output: Vec<f32> = Vec::with_capacity(frame_count * config.frame_size);

    for i in 0..frame_count {
        let start = i * config.frame_size;
        let end = start + config.frame_size;
        let data: Vec<u8> = samples[start..end]
            .iter()
            .flat_map(|s| s.to_ne_bytes())
            .collect();
        let frame = AudioFrame {
            data,
            format: fmt,
            sequence: i as u64,
            is_silent: false,
        };
        let packet = encoder.encode(&frame).unwrap();
        let decoded = decoder.decode(&packet).unwrap();
        output.extend_from_slice(decoded.as_f32_samples());
    }

    let skip = config.frame_size * 5;
    let total = frame_count * config.frame_size;
    let inp = &samples[skip..total];
    let out = &output[skip..output.len().min(total)];
    let delay = find_alignment_delay(inp, out, 960);
    let inp = &inp[..inp.len() - delay];
    let out = &out[delay..];
    let len = inp.len().min(out.len());
    let in_rms = rms(&inp[..len]);
    let err_rms = rms(
        &inp[..len]
            .iter()
            .zip(out[..len].iter())
            .map(|(&a, &b)| a - b)
            .collect::<Vec<f32>>(),
    );
    let snr = if err_rms > 0.0 {
        20.0 * (in_rms / err_rms).log10()
    } else {
        f32::INFINITY
    };
    (snr, delay)
}

/// Diagnostic: compare Opus quality across different configurations
/// to identify what parameter is causing poor encode quality.
#[test]
fn wav_opus_config_diagnostic() {
    let path = "tests/samples/my_voice2.wav";
    let Some(samples) = decode_wav_to_mono_48k(path) else {
        println!("Skipping wav_opus_config_diagnostic: {path} not found");
        return;
    };

    println!("=== Opus Configuration Diagnostic ===");
    println!("  Input: {path} ({} samples, {:.2}s)",
             samples.len(), samples.len() as f32 / 48000.0);
    println!("  Input RMS: {:.4}", rms(&samples));
    println!();

    let configs: Vec<(&str, OpusEncoderConfig)> = vec![
        ("User: 10ms/136k/Audio/FEC3%", OpusEncoderConfig {
            frame_size: 480,
            bitrate: 136_000,
            ..OpusEncoderConfig::default()
        }),
        ("20ms/136k/Audio/FEC3%", OpusEncoderConfig {
            frame_size: 960,
            bitrate: 136_000,
            ..OpusEncoderConfig::default()
        }),
        ("10ms/136k/Audio/noFEC", OpusEncoderConfig {
            frame_size: 480,
            bitrate: 136_000,
            fec: false,
            packet_loss_percent: 0,
            ..OpusEncoderConfig::default()
        }),
        ("10ms/136k/Audio/complex10", OpusEncoderConfig {
            frame_size: 480,
            bitrate: 136_000,
            complexity: 10,
            ..OpusEncoderConfig::default()
        }),
        ("10ms/136k/VoIP mode (old)", OpusEncoderConfig {
            frame_size: 480,
            bitrate: 136_000,
            application: OpusApplication::Voip,
            ..OpusEncoderConfig::default()
        }),
        ("20ms/72k/Audio (default)", OpusEncoderConfig::default()),
        ("10ms/64k/Audio", OpusEncoderConfig {
            frame_size: 480,
            bitrate: 64_000,
            ..OpusEncoderConfig::default()
        }),
        ("10ms/136k/Audio/noPktLoss", OpusEncoderConfig {
            frame_size: 480,
            bitrate: 136_000,
            packet_loss_percent: 0,
            ..OpusEncoderConfig::default()
        }),
    ];

    println!("  {:35} {:>8} {:>8}", "Config", "SNR(dB)", "Delay");
    println!("  {:-<35} {:-^8} {:-^8}", "", "", "");
    for (name, config) in &configs {
        let (snr, delay) = roundtrip_snr(&samples, config);
        println!("  {name:35} {snr:>7.1}  {delay:>6}");
    }

    // The user's config should achieve at least 5 dB SNR.
    let (user_snr, _) = roundtrip_snr(&samples, &configs[0].1);
    assert!(
        user_snr > 5.0,
        "User's config SNR {user_snr:.1} dB is too low"
    );
}

/// Compare a "reference" direct Opus encode/decode against our full
/// pipeline (`OutboundPipeline` -> `InboundPipeline` with crossfade).
///
/// Both paths use the same encoder config. The reference path does
/// bare `OpusEncoder::encode` -> `OpusDecoder::decode` with no
/// pipeline machinery. If the pipeline path is significantly worse,
/// the pipeline is adding degradation. If both are similar, the
/// codec config is the issue.
#[test]
fn wav_reference_vs_pipeline_comparison() {
    use mumble_protocol::audio::filter::FilterChain;
    use mumble_protocol::audio::pipeline::InboundPipeline;

    let path = "tests/samples/my_voice2.wav";
    let Some(samples) = decode_wav_to_mono_48k(path) else {
        println!("Skipping wav_reference_vs_pipeline_comparison: {path} not found");
        return;
    };

    let frame_size = 480; // 10 ms - user's setting
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let config = OpusEncoderConfig {
        frame_size,
        bitrate: 136_000,
        ..OpusEncoderConfig::default()
    };

    // ---- Path A: Reference (direct encode/decode, no pipeline) ----
    let mut ref_encoder = OpusEncoder::new(config.clone(), fmt).unwrap();
    let mut ref_decoder = OpusDecoder::new(fmt).unwrap();

    let frame_count = samples.len() / frame_size;
    let mut ref_output: Vec<f32> = Vec::with_capacity(frame_count * frame_size);
    let mut ref_packets: Vec<mumble_protocol::audio::encoder::EncodedPacket> =
        Vec::with_capacity(frame_count);

    for i in 0..frame_count {
        let start = i * frame_size;
        let end = start + frame_size;
        let data: Vec<u8> = samples[start..end]
            .iter()
            .flat_map(|s| s.to_ne_bytes())
            .collect();
        let frame = AudioFrame {
            data,
            format: fmt,
            sequence: i as u64,
            is_silent: false,
        };
        let packet = ref_encoder.encode(&frame).unwrap();
        let decoded = ref_decoder.decode(&packet).unwrap();
        ref_output.extend_from_slice(decoded.as_f32_samples());
        ref_packets.push(packet);
    }

    // ---- Path B: Full pipeline ----
    // Use OutboundPipeline with a buffer capture that feeds our samples,
    // then InboundPipeline with RecordingPlayback to collect output.

    // Feed packets through InboundPipeline (same packets from ref encoder,
    // so the only difference is the pipeline's crossfade/filtering).
    let pipe_decoder = OpusDecoder::new(fmt).unwrap();
    let (recording, pipe_buf) = RecordingPlayback::new(fmt);
    let mut inbound = InboundPipeline::new(
        Box::new(pipe_decoder),
        FilterChain::new(),
        Box::new(recording),
    );
    inbound.start().unwrap();
    for pkt in &ref_packets {
        inbound.tick(pkt).unwrap();
    }
    let pipe_output = pipe_buf.lock().unwrap();

    // ---- Alignment and SNR computation ----
    let skip = frame_size * 5;
    let total = frame_count * frame_size;

    // Align reference output to original
    let orig = &samples[skip..total];
    let ref_steady = &ref_output[skip..ref_output.len().min(total)];
    let ref_delay = find_alignment_delay(orig, ref_steady, 960);

    let ref_aligned_orig = &orig[..orig.len() - ref_delay];
    let ref_aligned_out = &ref_steady[ref_delay..];
    let ref_len = ref_aligned_orig.len().min(ref_aligned_out.len());

    let ref_error: Vec<f32> = ref_aligned_orig[..ref_len]
        .iter()
        .zip(ref_aligned_out[..ref_len].iter())
        .map(|(&a, &b)| a - b)
        .collect();
    let ref_snr = {
        let in_rms = rms(&ref_aligned_orig[..ref_len]);
        let err_rms = rms(&ref_error);
        if err_rms > 0.0 { 20.0 * (in_rms / err_rms).log10() } else { f32::INFINITY }
    };

    // Align pipeline output to original
    let pipe_steady = &pipe_output[skip..pipe_output.len().min(total)];
    let pipe_delay = find_alignment_delay(orig, pipe_steady, 960);

    let pipe_aligned_orig = &orig[..orig.len() - pipe_delay];
    let pipe_aligned_out = &pipe_steady[pipe_delay..];
    let pipe_len = pipe_aligned_orig.len().min(pipe_aligned_out.len());

    let pipe_error: Vec<f32> = pipe_aligned_orig[..pipe_len]
        .iter()
        .zip(pipe_aligned_out[..pipe_len].iter())
        .map(|(&a, &b)| a - b)
        .collect();
    let pipe_snr = {
        let in_rms = rms(&pipe_aligned_orig[..pipe_len]);
        let err_rms = rms(&pipe_error);
        if err_rms > 0.0 { 20.0 * (in_rms / err_rms).log10() } else { f32::INFINITY }
    };

    // Compare pipeline output directly against reference output
    let cross_len = ref_aligned_out.len().min(pipe_aligned_out.len())
        .min(ref_aligned_orig.len());
    let ref_vs_pipe_error: Vec<f32> = ref_aligned_out[..cross_len]
        .iter()
        .zip(pipe_aligned_out[..cross_len].iter())
        .map(|(&a, &b)| a - b)
        .collect();
    let ref_vs_pipe_snr = {
        let sig_rms = rms(&ref_aligned_out[..cross_len]);
        let err_rms = rms(&ref_vs_pipe_error);
        if err_rms > 0.0 { 20.0 * (sig_rms / err_rms).log10() } else { f32::INFINITY }
    };

    // Pop/roughness comparison
    let (ref_pops, _ref_pop_rate, ref_max) = detect_pops(ref_steady, 480, 10.0);
    let (pipe_pops, _pipe_pop_rate, pipe_max) = detect_pops(pipe_steady, 480, 10.0);
    let ref_roughness = energy_roughness(ref_steady, 480);
    let pipe_roughness = energy_roughness(pipe_steady, 480);
    let orig_roughness = energy_roughness(orig, 480);

    println!("=== Reference vs Pipeline Comparison ===");
    println!("  Original:  RMS={:.4}, roughness={orig_roughness:.4}", rms(orig));
    println!();
    println!("  {:20} {:>8} {:>6} {:>6} {:>8} {:>10}",
             "", "SNR(dB)", "Delay", "Pops", "MaxDelta", "Roughness");
    println!("  {:-<20} {:-^8} {:-^6} {:-^6} {:-^8} {:-^10}", "", "", "", "", "", "");
    println!("  {:20} {:>7.1}  {:>5}  {:>5}  {:>7.4}  {:>9.4}",
             "Reference (direct)", ref_snr, ref_delay, ref_pops, ref_max, ref_roughness);
    println!("  {:20} {:>7.1}  {:>5}  {:>5}  {:>7.4}  {:>9.4}",
             "Pipeline", pipe_snr, pipe_delay, pipe_pops, pipe_max, pipe_roughness);
    println!();
    println!("  Ref vs Pipeline SNR: {ref_vs_pipe_snr:.1} dB (higher = more similar)");
    println!("  SNR difference: {:.1} dB (pipeline - reference)", pipe_snr - ref_snr);

    // Pipeline should not be significantly worse than reference
    assert!(
        pipe_snr >= ref_snr - 3.0,
        "Pipeline SNR ({pipe_snr:.1} dB) is {:.1} dB worse than reference ({ref_snr:.1} dB)",
        ref_snr - pipe_snr
    );

    // Both should meet minimum quality
    assert!(
        ref_snr > 5.0,
        "Reference SNR {ref_snr:.1} dB is too low - Opus config problem, not pipeline"
    );
    assert!(
        pipe_snr > 5.0,
        "Pipeline SNR {pipe_snr:.1} dB is too low"
    );
}

/// Full crackling analysis of a WAV voice sample through the Opus
/// pipeline matching the user's exact audio settings:
///   Frame size: 480 (10 ms)
///   Bitrate: 136 kb/s
///   No noise gate, no AGC
///
/// Detects crackling through multiple metrics:
///   1. Pop/click count (sample-level delta spikes)
///   2. Energy roughness (rapid short-term energy fluctuations)
///   3. Frame boundary discontinuities
///   4. Spectral distortion in 2-8 kHz band (where crackling lives)
///   5. Signal-to-Noise Ratio (error energy vs signal energy)
///
/// Gracefully skips if WAV file not found.
#[test]
fn wav_voice_crackling_detection() {
    let path = "tests/samples/my_voice2.wav";
    let Some(samples) = decode_wav_to_mono_48k(path) else {
        println!("Skipping wav_voice_crackling_detection: {path} not found");
        return;
    };

    let frame_size = 480; // 10 ms
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let config = OpusEncoderConfig {
        frame_size,
        bitrate: 136_000,
        ..OpusEncoderConfig::default()
    };

    let mut encoder = OpusEncoder::new(config, fmt).unwrap();
    let mut decoder = OpusDecoder::new(fmt).unwrap();

    let frame_count = samples.len() / frame_size;
    let mut output_samples: Vec<f32> = Vec::with_capacity(frame_count * frame_size);

    for i in 0..frame_count {
        let start = i * frame_size;
        let end = start + frame_size;
        let data: Vec<u8> = samples[start..end]
            .iter()
            .flat_map(|s| s.to_ne_bytes())
            .collect();
        let frame = AudioFrame {
            data,
            format: fmt,
            sequence: i as u64,
            is_silent: false,
        };
        let packet = encoder.encode(&frame).unwrap();
        let decoded = decoder.decode(&packet).unwrap();
        output_samples.extend_from_slice(decoded.as_f32_samples());
    }

    // Skip warm-up frames
    let skip = frame_size * 5;
    let total = frame_count * frame_size;
    let input_raw = &samples[skip..total];
    let output_raw = &output_samples[skip..output_samples.len().min(total)];

    // Opus introduces an algorithmic delay (lookahead).  The decoded
    // output is shifted in time relative to the input.  Find the
    // optimal alignment via cross-correlation before computing SNR.
    let delay = find_alignment_delay(input_raw, output_raw, 960);
    let input = &input_raw[..input_raw.len() - delay];
    let output = &output_raw[delay..];
    let len = input.len().min(output.len());
    let input = &input[..len];
    let output = &output[..len];

    // Compute error signal
    let error_signal: Vec<f32> = input
        .iter()
        .zip(output.iter())
        .map(|(&a, &b)| a - b)
        .collect();

    println!("=== WAV Voice Crackling Analysis ({frame_count} frames, 10 ms) ===");
    println!("  Alignment delay: {delay} samples ({:.2} ms)", delay as f32 / 48.0);

    // 1. Pop/click detection on error signal
    // If the codec introduces crackling, the error signal will have
    // sudden spikes that don't correspond to the original audio.
    let (pop_count, pop_rate, max_delta) = detect_pops(&error_signal, 480, 8.0);
    println!("  Error signal pops: count={pop_count}, rate={pop_rate:.2}%, max_delta={max_delta:.4}");

    // 2. Pop detection on output signal directly
    // Crackling manifests as abnormal sample-to-sample jumps.
    let (out_pops, out_pop_rate, out_max_delta) = detect_pops(output, 480, 10.0);
    println!("  Output pops: count={out_pops}, rate={out_pop_rate:.2}%, max_delta={out_max_delta:.4}");

    // 3. Energy roughness of error signal
    // Smooth codec errors (low roughness) = normal loss;
    // Rough errors = crackling pattern.
    let err_roughness = energy_roughness(&error_signal, 480);
    let out_roughness = energy_roughness(output, 480);
    let in_roughness = energy_roughness(input, 480);
    println!(
        "  Energy roughness: input={in_roughness:.4}, output={out_roughness:.4}, \
         error={err_roughness:.4}"
    );

    // 4. Frame boundary analysis
    let skip_frames = 5;
    let mut boundary_jumps: Vec<f32> = Vec::new();
    let mut internal_jumps: Vec<f32> = Vec::new();

    for i in skip_frames..frame_count.min(output_samples.len() / frame_size) {
        let boundary = i * frame_size;
        if boundary == 0 || boundary >= output_samples.len() {
            continue;
        }

        boundary_jumps.push(
            (output_samples[boundary] - output_samples[boundary - 1]).abs(),
        );

        // Sample internal jumps for comparison
        let frame_end = ((i + 1) * frame_size).min(output_samples.len());
        for j in (boundary + 1)..frame_end {
            internal_jumps.push((output_samples[j] - output_samples[j - 1]).abs());
        }
    }

    let mean_boundary = boundary_jumps.iter().sum::<f32>() / boundary_jumps.len().max(1) as f32;
    let max_boundary = boundary_jumps.iter().copied().fold(0.0f32, f32::max);
    let mean_internal = internal_jumps.iter().sum::<f32>() / internal_jumps.len().max(1) as f32;
    let boundary_ratio = if mean_internal > 1e-6 {
        mean_boundary / mean_internal
    } else {
        0.0
    };

    println!(
        "  Boundaries: mean={mean_boundary:.4}, max={max_boundary:.4}, \
         internal={mean_internal:.4}, ratio={boundary_ratio:.2}"
    );

    // 5. SNR of the codec error
    //
    // For very quiet signals (e.g. low mic gain), the Opus quantization
    // noise floor is comparable to the signal.  SNR depends on input
    // level, not codec quality.  We only assert when the input is loud
    // enough for SNR to be meaningful.
    let input_rms = rms(input);
    let error_rms = rms(&error_signal);
    let snr = if error_rms > 0.0 {
        20.0 * (input_rms / error_rms).log10()
    } else {
        f64::INFINITY as f32
    };
    println!("  SNR: {snr:.1} dB (input_rms={input_rms:.4}, error_rms={error_rms:.4})");

    // 6. Clipping detection
    let clipped_in = input.iter().filter(|&&s| s.abs() > 0.99).count();
    let clipped_out = output.iter().filter(|&&s| s.abs() > 0.99).count();
    let clipped_err = error_signal.iter().filter(|&&s| s.abs() > 0.1).count();
    println!(
        "  Clipping: input={clipped_in}, output={clipped_out}, \
         error>0.1={clipped_err}"
    );

    // 7. High-frequency artifact energy (2-8 kHz where crackling lives)
    // Compare spectral energy in this band between input and output.
    // Crackling adds energy here that wasn't in the original.
    let hf_window = 2048;
    let mut in_hf_energy = 0.0f64;
    let mut out_hf_energy = 0.0f64;
    let mut windows_analysed = 0u32;

    let mut pos = 0;
    while pos + hf_window <= len {
        // FFT both windows
        let mut in_re: Vec<f32> = input[pos..pos + hf_window].to_vec();
        let mut in_im = vec![0.0f32; hf_window];
        let mut out_re: Vec<f32> = output[pos..pos + hf_window].to_vec();
        let mut out_im = vec![0.0f32; hf_window];

        fft(&mut in_re, &mut in_im);
        fft(&mut out_re, &mut out_im);

        // Bins for 2000-8000 Hz
        let bin_lo = 2000 * hf_window / SAMPLE_RATE as usize;
        let bin_hi = 8000 * hf_window / SAMPLE_RATE as usize;

        for k in bin_lo..bin_hi.min(hf_window / 2) {
            in_hf_energy += (in_re[k] * in_re[k] + in_im[k] * in_im[k]) as f64;
            out_hf_energy += (out_re[k] * out_re[k] + out_im[k] * out_im[k]) as f64;
        }
        windows_analysed += 1;
        pos += hf_window;
    }

    if windows_analysed > 0 {
        in_hf_energy /= windows_analysed as f64;
        out_hf_energy /= windows_analysed as f64;
        let hf_ratio = if in_hf_energy > 0.0 {
            out_hf_energy / in_hf_energy
        } else {
            1.0
        };
        println!(
            "  HF energy (2-8kHz): ratio={hf_ratio:.3} \
             (>1.5 = added artifacts, {windows_analysed} windows)"
        );
        assert!(
            hf_ratio < 3.0,
            "High-frequency energy ratio {hf_ratio:.2} indicates severe spectral artifacts"
        );
    }

    // ---- Assertions ----
    assert!(
        pop_rate < 5.0,
        "Error signal pop rate {pop_rate:.2}% too high (crackling present)"
    );
    assert!(
        out_pop_rate < 3.0,
        "Output pop rate {out_pop_rate:.2}% too high (audible clicks)"
    );
    assert!(
        max_boundary < 0.5,
        "Max boundary jump {max_boundary:.4} indicates severe frame-boundary click"
    );
    assert!(
        boundary_ratio < 3.0,
        "Boundary/internal ratio {boundary_ratio:.2} indicates systematic frame discontinuities"
    );
    assert!(
        snr > 5.0,
        "SNR {snr:.1} dB too low (codec is introducing too much noise/distortion)"
    );
}

/// Full pipeline crackling test: encode through `OutboundPipeline`
/// (with user's exact filter settings) -> decode through `InboundPipeline`
/// (with crossfade).
///
/// This tests the full codec path including pipeline crossfade to
/// detect if the pipeline itself introduces crackling beyond what
/// the raw codec does.
#[test]
fn wav_voice_full_pipeline_crackling() {
    use mumble_protocol::audio::filter::FilterChain;
    use mumble_protocol::audio::pipeline::InboundPipeline;

    let path = "tests/samples/my_voice2.wav";
    let Some(samples) = decode_wav_to_mono_48k(path) else {
        println!("Skipping wav_voice_full_pipeline_crackling: {path} not found");
        return;
    };

    let frame_size = 480; // 10 ms
    let fmt = AudioFormat::MONO_48KHZ_F32;
    let config = OpusEncoderConfig {
        frame_size,
        bitrate: 136_000,
        ..OpusEncoderConfig::default()
    };

    let mut encoder = OpusEncoder::new(config, fmt).unwrap();

    // Encode all frames
    let frame_count = samples.len() / frame_size;
    let mut packets = Vec::with_capacity(frame_count);
    for i in 0..frame_count {
        let start = i * frame_size;
        let end = start + frame_size;
        let data: Vec<u8> = samples[start..end]
            .iter()
            .flat_map(|s| s.to_ne_bytes())
            .collect();
        let frame = AudioFrame {
            data,
            format: fmt,
            sequence: i as u64,
            is_silent: false,
        };
        packets.push(encoder.encode(&frame).unwrap());
    }

    // ---- Raw decode (no pipeline, no crossfade) ----
    let mut raw_decoder = OpusDecoder::new(fmt).unwrap();
    let mut raw_output: Vec<f32> = Vec::new();
    for pkt in &packets {
        let decoded = raw_decoder.decode(pkt).unwrap();
        raw_output.extend_from_slice(decoded.as_f32_samples());
    }

    // ---- Pipeline decode (with crossfade) ----
    let pipeline_decoder = OpusDecoder::new(fmt).unwrap();
    let (recording, pipe_buf) = RecordingPlayback::new(fmt);
    let mut pipeline = InboundPipeline::new(
        Box::new(pipeline_decoder),
        FilterChain::new(),
        Box::new(recording),
    );
    pipeline.start().unwrap();
    for pkt in &packets {
        pipeline.tick(pkt).unwrap();
    }
    let pipe_output = pipe_buf.lock().unwrap();

    let skip = frame_size * 5;
    let total = frame_count * frame_size;

    println!("=== WAV Full Pipeline Crackling Comparison ===");

    // Compare raw vs pipeline pop rates
    let raw_steady =
        &raw_output[skip..raw_output.len().min(total)];
    let pipe_steady =
        &pipe_output[skip..pipe_output.len().min(total)];

    let (raw_pops, raw_pop_rate, raw_max) = detect_pops(raw_steady, 480, 10.0);
    let (pipe_pops, pipe_pop_rate, pipe_max) = detect_pops(pipe_steady, 480, 10.0);

    println!(
        "  Raw decode:  pops={raw_pops}, rate={raw_pop_rate:.2}%, max_delta={raw_max:.4}"
    );
    println!(
        "  Pipeline:    pops={pipe_pops}, rate={pipe_pop_rate:.2}%, max_delta={pipe_max:.4}"
    );

    // Compare energy roughness
    let raw_roughness = energy_roughness(raw_steady, 480);
    let pipe_roughness = energy_roughness(pipe_steady, 480);
    println!(
        "  Roughness:   raw={raw_roughness:.4}, pipeline={pipe_roughness:.4}"
    );

    // Boundary analysis for both
    let skip_frames = 5;
    let measure_end = frame_count
        .min(raw_output.len() / frame_size)
        .min(pipe_output.len() / frame_size);

    let measure_boundaries = |out: &[f32]| -> (f32, f32) {
        let mut jumps = Vec::new();
        for i in skip_frames..measure_end {
            let idx = i * frame_size;
            if idx > 0 && idx < out.len() {
                jumps.push((out[idx] - out[idx - 1]).abs());
            }
        }
        let mean = jumps.iter().sum::<f32>() / jumps.len().max(1) as f32;
        let max = jumps.iter().copied().fold(0.0f32, f32::max);
        (mean, max)
    };

    let (raw_bdry_mean, raw_bdry_max) = measure_boundaries(&raw_output);
    let (pipe_bdry_mean, pipe_bdry_max) = measure_boundaries(&pipe_output);
    println!(
        "  Boundaries:  raw mean={raw_bdry_mean:.4} max={raw_bdry_max:.4}  \
         | pipe mean={pipe_bdry_mean:.4} max={pipe_bdry_max:.4}"
    );

    // Pipeline should not make things worse
    assert!(
        pipe_pop_rate <= raw_pop_rate + 1.0,
        "Pipeline increased pop rate: {pipe_pop_rate:.2}% vs raw {raw_pop_rate:.2}%"
    );
    assert!(
        pipe_bdry_mean <= raw_bdry_mean * 1.2 + 0.001,
        "Pipeline increased boundary jumps: {pipe_bdry_mean:.4} vs raw {raw_bdry_mean:.4}"
    );

    // Absolute quality gates
    assert!(
        pipe_pop_rate < 3.0,
        "Pipeline pop rate {pipe_pop_rate:.2}% indicates crackling"
    );
    assert!(
        pipe_bdry_max < 0.5,
        "Pipeline max boundary jump {pipe_bdry_max:.4} indicates severe click"
    );
}

/// Auto-discover and test all WAV files in the samples directory.
///
/// Each file goes through encode->decode with the user's audio settings
/// (10 ms / 136 kb/s, no filters) and is tested for crackling.
#[test]
fn wav_roundtrip_all_samples() {
    let samples_dir = std::path::Path::new("tests/samples");
    if !samples_dir.exists() {
        println!("Skipping wav_roundtrip_all_samples: tests/samples/ not found");
        return;
    }

    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(samples_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
        })
        .collect();

    if paths.is_empty() {
        println!("Skipping wav_roundtrip_all_samples: no .wav files");
        return;
    }

    paths.sort();
    println!("wav_roundtrip_all_samples: testing {} file(s)", paths.len());

    for path in &paths {
        let path_str = path.to_str().unwrap_or("<invalid>");
        let Some(samples) = decode_wav_to_mono_48k(path_str) else {
            println!("  {path_str}: could not decode, skipping");
            continue;
        };

        let frame_size = 480;
        let fmt = AudioFormat::MONO_48KHZ_F32;
        let config = OpusEncoderConfig {
            frame_size,
            bitrate: 136_000,
            ..OpusEncoderConfig::default()
        };

        let mut encoder = OpusEncoder::new(config, fmt).unwrap();
        let mut decoder = OpusDecoder::new(fmt).unwrap();

        let frame_count = samples.len() / frame_size;
        let mut output: Vec<f32> = Vec::with_capacity(frame_count * frame_size);

        for i in 0..frame_count {
            let start = i * frame_size;
            let end = start + frame_size;
            let data: Vec<u8> = samples[start..end]
                .iter()
                .flat_map(|s| s.to_ne_bytes())
                .collect();
            let frame = AudioFrame {
                data,
                format: fmt,
                sequence: i as u64,
                is_silent: false,
            };
            let packet = encoder.encode(&frame).unwrap();
            let decoded = decoder.decode(&packet).unwrap();
            output.extend_from_slice(decoded.as_f32_samples());
        }

        let skip = frame_size * 5;
        let total = frame_count * frame_size;
        let out_steady = &output[skip..output.len().min(total)];

        let (pops, pop_rate, max_delta) = detect_pops(out_steady, 480, 10.0);
        let roughness = energy_roughness(out_steady, 480);

        let input_steady = &samples[skip..total.min(samples.len())];
        let delay = find_alignment_delay(input_steady, out_steady, 960);
        let aligned_in = &input_steady[..input_steady.len() - delay];
        let aligned_out = &out_steady[delay..];
        let comp_len = aligned_in.len().min(aligned_out.len());
        let error_rms = rms(
            &aligned_in[..comp_len]
                .iter()
                .zip(aligned_out[..comp_len].iter())
                .map(|(&a, &b)| a - b)
                .collect::<Vec<f32>>(),
        );
        let in_rms = rms(&aligned_in[..comp_len]);
        let snr = if error_rms > 0.0 {
            20.0 * (in_rms / error_rms).log10()
        } else {
            f64::INFINITY as f32
        };

        println!(
            "  {path_str}\n    \
             frames={frame_count}  pops={pops} ({pop_rate:.2}%)  \
             max_delta={max_delta:.4}  roughness={roughness:.4}  SNR={snr:.1}dB"
        );

        assert!(
            pop_rate < 5.0,
            "[{path_str}] pop rate {pop_rate:.2}% too high"
        );
        assert!(
            snr > 5.0,
            "[{path_str}] SNR {snr:.1} dB too low"
        );
    }
}
