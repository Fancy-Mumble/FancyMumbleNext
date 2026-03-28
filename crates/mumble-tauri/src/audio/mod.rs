//! Platform-specific audio capture and playback implementations.
//!
//! Each platform provides types that implement the protocol library's
//! [`AudioCapture`](mumble_protocol::audio::capture::AudioCapture) and
//! [`AudioPlayback`](mumble_protocol::audio::playback::AudioPlayback) traits,
//! allowing the pipeline infrastructure to drive real hardware without
//! knowing which OS audio API is in use.
//!
//! The [`AudioDeviceFactory`] trait abstracts over platform-specific
//! device creation. Each platform module provides its own implementation,
//! and [`PlatformAudioFactory`] is a type alias for the current platform's
//! factory, so callers never need `cfg` gates.

use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use mumble_protocol::audio::capture::AudioCapture;
use mumble_protocol::audio::mixer::{SpeakerBuffers, SpeakerVolumes};
use mumble_protocol::error::Result;

#[cfg(not(target_os = "android"))]
mod desktop;

#[cfg(target_os = "android")]
mod android;

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

#[cfg(not(target_os = "android"))]
pub use desktop::CpalAudioFactory as PlatformAudioFactory;

#[cfg(target_os = "android")]
pub use android::OboeAudioFactory as PlatformAudioFactory;
