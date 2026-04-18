//! Platform-agnostic screen capture trait.

use std::fmt;

/// Raw RGBA frame captured from the screen.
pub struct CapturedFrame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Raw pixel data in RGBA format (4 bytes per pixel), row-major.
    pub data: Vec<u8>,
}

/// Opaque handle for an active capture session.
///
/// Dropping the handle should stop the capture.
pub trait CaptureSession: Send + 'static {}

/// Errors that can occur during screen capture.
#[derive(Debug)]
pub enum CaptureError {
    /// The user denied the screen-share permission dialog.
    /// Used by platform implementations (e.g. Android `MediaProjection`).
    #[allow(dead_code, reason = "will be used by Android platform implementation")]
    PermissionDenied,
    /// No suitable screen/monitor was found.
    #[allow(dead_code, reason = "used by platform implementations that enumerate monitors")]
    NoScreen,
    /// Platform-specific error.
    Platform(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied => write!(f, "screen capture permission denied"),
            Self::NoScreen => write!(f, "no screen available for capture"),
            Self::Platform(msg) => write!(f, "capture error: {msg}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Callback invoked for each captured frame.
pub type FrameCallback = Box<dyn Fn(CapturedFrame) + Send + Sync + 'static>;

/// Platform-independent screen-capture interface.
///
/// Each platform provides its own implementation (e.g. XDG Desktop Portal
/// on Linux/Wayland, `MediaProjection` on Android).
pub trait ScreenCapture: Send + Sync + 'static {
    /// The session handle returned by [`start`](Self::start).
    type Session: CaptureSession;

    /// Begin capturing the screen.
    ///
    /// `on_frame` is called for every captured frame from a background task.
    /// Returns a session handle whose drop stops the capture.
    fn start(
        &self,
        on_frame: FrameCallback,
    ) -> impl std::future::Future<Output = Result<Self::Session, CaptureError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_error_display() {
        assert_eq!(
            CaptureError::PermissionDenied.to_string(),
            "screen capture permission denied"
        );
        assert_eq!(
            CaptureError::NoScreen.to_string(),
            "no screen available for capture"
        );
        assert_eq!(
            CaptureError::Platform("test".into()).to_string(),
            "capture error: test"
        );
    }

    #[test]
    fn captured_frame_layout() {
        let frame = CapturedFrame {
            width: 2,
            height: 2,
            data: vec![0; 2 * 2 * 4],
        };
        assert_eq!(frame.data.len(), (frame.width as usize) * (frame.height as usize) * 4);
    }
}
