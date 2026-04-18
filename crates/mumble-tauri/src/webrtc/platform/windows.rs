//! Windows screen capture stub.
//!
//! On Windows, `WebView2` (Chromium) supports `getDisplayMedia` natively so
//! the browser-based WebRTC path is preferred.  This stub exists for future
//! use if a Rust-native pipeline is desired (e.g. DXGI Desktop Duplication).

use crate::webrtc::capture::{CaptureError, CaptureSession, FrameCallback, ScreenCapture};

/// Windows screen capture (placeholder).
pub struct WindowsCapture {
    _private: (),
}

impl WindowsCapture {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

/// Active capture session handle.
pub struct WindowsSession {
    _private: (),
}

impl CaptureSession for WindowsSession {}

impl ScreenCapture for WindowsCapture {
    type Session = WindowsSession;

    fn start(
        &self,
        _on_frame: FrameCallback,
    ) -> impl std::future::Future<Output = Result<WindowsSession, CaptureError>> + Send {
        async {
            // TODO: Implement via DXGI Desktop Duplication API
            Err(CaptureError::Platform(
                "Windows native capture not yet implemented — use browser WebRTC".into(),
            ))
        }
    }
}
