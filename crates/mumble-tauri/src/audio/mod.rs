//! Platform-specific audio capture and playback implementations.
//!
//! Each platform provides types that implement the protocol library's
//! [`AudioCapture`](mumble_protocol::audio::capture::AudioCapture) and
//! [`AudioPlayback`](mumble_protocol::audio::playback::AudioPlayback) traits,
//! allowing the pipeline infrastructure to drive real hardware without
//! knowing which OS audio API is in use.
//!
//! On desktop, two backends are available:
//!
//! * **rodio** (default) - higher-level push-based API with built-in
//!   mixing, sample-rate conversion, and background threading.
//! * **cpal** (legacy) - low-level callback-based API exposed as a
//!   fallback via the advanced settings toggle.
//!
//! The [`AudioDeviceFactory`] trait abstracts over platform-specific
//! device creation. [`PlatformAudioFactory`] dispatches to the
//! currently-selected backend at runtime so callers never need `cfg`
//! gates or backend checks.

use std::sync::atomic::AtomicU32;
#[cfg(not(target_os = "android"))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mumble_protocol::audio::capture::AudioCapture;
use mumble_protocol::audio::mixer::{SpeakerBuffers, SpeakerVolumes};
use mumble_protocol::error::Result;

#[cfg(not(target_os = "android"))]
mod desktop;

#[cfg(not(target_os = "android"))]
mod rodio_desktop;

#[cfg(target_os = "android")]
mod android;

// -- Backend selection (desktop only) --------------------------------

/// When `true`, the rodio backend is used; when `false`, the legacy
/// cpal backend is used. Defaults to `true` (rodio).
#[cfg(not(target_os = "android"))]
static USE_RODIO_BACKEND: AtomicBool = AtomicBool::new(true);

/// Switch the desktop audio backend at runtime.
///
/// `true` selects the rodio backend (default), `false` selects the
/// legacy cpal backend.  The change takes effect on the next
/// `create_capture` / `create_mixing_playback` call (i.e. on the next
/// connect or voice toggle).
#[cfg(not(target_os = "android"))]
pub fn set_use_rodio_backend(use_rodio: bool) {
    USE_RODIO_BACKEND.store(use_rodio, Ordering::Relaxed);
}

/// Returns `true` if the rodio backend is currently selected.
#[cfg(not(target_os = "android"))]
pub fn is_rodio_backend() -> bool {
    USE_RODIO_BACKEND.load(Ordering::Relaxed)
}

/// On Android there is only one backend, so this is a no-op.
#[cfg(target_os = "android")]
pub fn set_use_rodio_backend(_use_rodio: bool) {}

/// On Android there is only one backend; always returns `true`.
#[cfg(target_os = "android")]
pub fn is_rodio_backend() -> bool {
    true
}

// -- Traits ----------------------------------------------------------

/// Abstract factory for creating platform-specific audio devices.
///
/// Each platform module implements this trait on a zero-sized struct.
/// Consumer code uses [`PlatformAudioFactory`] and never touches
/// `cfg` gates directly.
pub trait AudioDeviceFactory {
    /// Create a capture (microphone) device.
    ///
    /// `device_name` selects a specific input device; platforms that do
    /// not support device selection (e.g. Android) ignore it.
    fn create_capture(
        device_name: Option<&str>,
        frame_size: usize,
        volume: Arc<AtomicU32>,
    ) -> std::result::Result<Box<dyn AudioCapture>, String>;

    /// Create a mixing playback device that reads from per-speaker
    /// buffers, sums all active speakers, and outputs to hardware.
    fn create_mixing_playback(
        device_name: Option<&str>,
        volume: Arc<AtomicU32>,
        buffers: SpeakerBuffers,
        speaker_volumes: SpeakerVolumes,
    ) -> std::result::Result<Box<dyn MixingPlayback>, String>;
}

/// A playback device that mixes multiple speakers in its audio callback.
///
/// Unlike [`AudioPlayback`], this device does not receive frames via
/// `write_frame` — decoded samples are written into [`SpeakerBuffers`]
/// by the [`AudioMixer`](mumble_protocol::audio::mixer::AudioMixer),
/// and the callback reads + sums them directly.
pub trait MixingPlayback: Send + 'static {
    /// Start the output stream.
    fn start(&mut self) -> Result<()>;
    /// Stop the output stream.
    fn stop(&mut self) -> Result<()>;
}

// -- Platform factory ------------------------------------------------

/// Desktop: dispatches to rodio or cpal based on the runtime toggle.
#[cfg(not(target_os = "android"))]
pub struct PlatformAudioFactory;

#[cfg(not(target_os = "android"))]
impl AudioDeviceFactory for PlatformAudioFactory {
    fn create_capture(
        device_name: Option<&str>,
        frame_size: usize,
        volume: Arc<AtomicU32>,
    ) -> std::result::Result<Box<dyn AudioCapture>, String> {
        if USE_RODIO_BACKEND.load(Ordering::Relaxed) {
            rodio_desktop::RodioAudioFactory::create_capture(device_name, frame_size, volume)
        } else {
            desktop::CpalAudioFactory::create_capture(device_name, frame_size, volume)
        }
    }

    fn create_mixing_playback(
        device_name: Option<&str>,
        volume: Arc<AtomicU32>,
        buffers: SpeakerBuffers,
        speaker_volumes: SpeakerVolumes,
    ) -> std::result::Result<Box<dyn MixingPlayback>, String> {
        if USE_RODIO_BACKEND.load(Ordering::Relaxed) {
            rodio_desktop::RodioAudioFactory::create_mixing_playback(
                device_name, volume, buffers, speaker_volumes,
            )
        } else {
            desktop::CpalAudioFactory::create_mixing_playback(
                device_name, volume, buffers, speaker_volumes,
            )
        }
    }
}

#[cfg(target_os = "android")]
pub use android::OboeAudioFactory as PlatformAudioFactory;

/// Soft-clip a sample to the [-1.0, 1.0] range.
///
/// Samples within [-0.9, 0.9] pass through unchanged.  Beyond that
/// threshold the signal is smoothly compressed using `tanh` so it
/// asymptotically approaches +/-1.0 without ever exceeding it.  This
/// avoids the harsh distortion of hard clipping while preserving
/// dynamics for normal-level audio.
#[inline]
pub(crate) fn soft_clip(sample: f32) -> f32 {
    const KNEE: f32 = 0.9;
    if sample.abs() <= KNEE {
        return sample;
    }
    let sign = sample.signum();
    let excess = sample.abs() - KNEE;
    let compressed = KNEE + (1.0 - KNEE) * (excess / (1.0 - KNEE)).tanh();
    sign * compressed
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn soft_clip_passes_through_below_knee() {
        for &v in &[0.0, 0.5, -0.5, 0.89, -0.89] {
            assert_eq!(soft_clip(v), v, "below knee should pass through unchanged");
        }
    }

    #[test]
    fn soft_clip_compresses_above_knee() {
        let out = soft_clip(1.2);
        assert!(out > 0.9, "should be above knee: {out}");
        assert!(out < 1.0, "should be below 1.0: {out}");
    }

    #[test]
    fn soft_clip_never_exceeds_one() {
        for i in 1..100 {
            let v = i as f32;
            assert!(soft_clip(v).abs() <= 1.0, "soft_clip({v}) exceeded 1.0");
            assert!(soft_clip(-v).abs() <= 1.0, "soft_clip({}) exceeded 1.0", -v);
        }
    }

    #[test]
    fn soft_clip_is_symmetric() {
        for &v in &[0.95, 1.0, 1.5, 3.0] {
            let pos = soft_clip(v);
            let neg = soft_clip(-v);
            assert!(
                (pos + neg).abs() < 1e-6,
                "asymmetric: soft_clip({v})={pos}, soft_clip({})={neg}", -v
            );
        }
    }
}
