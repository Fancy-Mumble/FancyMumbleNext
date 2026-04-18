//! Linux screen capture stub.
//!
//! The actual capture pipeline lives in [`crate::webrtc::gstreamer_pipeline`]
//! which uses GStreamer's `pipewiresrc` directly. This module provides the
//! minimal `ScreenCapture` impl so that `platform::create_capturer()` (and
//! therefore `capabilities()`) reports native capture as available on Linux.

use crate::webrtc::capture::{
    CaptureError, CaptureSession, FrameCallback, ScreenCapture,
};

use super::CaptureSourceInfo;

/// Stub capturer that signals availability but delegates real work to
/// [`crate::webrtc::gstreamer_pipeline::GstScreenCapture`].
pub struct PortalCapture;

impl PortalCapture {
    /// Create a new stub capturer.
    pub fn new() -> Self {
        Self
    }

    /// Create a stub capturer (monitor index ignored).
    pub fn with_monitor(_index: usize) -> Self {
        Self
    }
}

/// Stub session handle.
pub struct PortalSession;

impl CaptureSession for PortalSession {}

impl ScreenCapture for PortalCapture {
    type Session = PortalSession;

    async fn start(&self, _on_frame: FrameCallback) -> Result<PortalSession, CaptureError> {
        Err(CaptureError::Platform(
            "use GstScreenCapture instead of the generic MJPEG path on Linux".into(),
        ))
    }
}

/// Source listing is handled by the XDG portal picker natively.
pub fn list_capture_sources() -> Result<Vec<CaptureSourceInfo>, String> {
    Ok(Vec::new())
}
