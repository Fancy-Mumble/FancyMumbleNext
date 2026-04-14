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
