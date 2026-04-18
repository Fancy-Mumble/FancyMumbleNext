//! macOS screen capture stub.
//!
//! On macOS, `WKWebView` supports `getDisplayMedia` natively so the
//! browser-based WebRTC path is preferred.  This stub exists for future use
//! if a Rust-native pipeline is desired (e.g. `SCScreenRecorder` /
//! `ScreenCaptureKit`).

use crate::webrtc::capture::{CaptureError, CaptureSession, FrameCallback, ScreenCapture};

/// macOS screen capture (placeholder).
pub struct MacCapture {
    _private: (),
}

impl MacCapture {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

/// Active capture session handle.
pub struct MacSession {
    _private: (),
}

impl CaptureSession for MacSession {}

impl ScreenCapture for MacCapture {
    type Session = MacSession;

    fn start(
        &self,
        _on_frame: FrameCallback,
    ) -> impl std::future::Future<Output = Result<MacSession, CaptureError>> + Send {
        async {
            // TODO: Implement via ScreenCaptureKit
            Err(CaptureError::Platform(
                "macOS native capture not yet implemented — use browser WebRTC".into(),
            ))
        }
    }
}
