//! Platform-specific screen capture implementations.

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod macos;

use std::future::Future;
use std::pin::Pin;

use crate::webrtc::capture::{CaptureError, FrameCallback, ScreenCapture};

/// Opaque platform capture wrapping the concrete implementation.
pub struct PlatformCapture {
    inner: Box<dyn ErasedCapture>,
}

/// Type-erased session returned by the platform capturer.
pub struct PlatformSession {
    _inner: Box<dyn std::any::Any + Send>,
}

impl crate::webrtc::capture::CaptureSession for PlatformSession {}

impl PlatformCapture {
    /// Start capturing. Delegates to the platform-specific implementation.
    pub async fn start(&self, on_frame: FrameCallback) -> Result<PlatformSession, CaptureError> {
        self.inner.start_erased(on_frame).await
    }
}

/// Create the screen capturer for the current platform.
///
/// Returns `Err` if the current platform has no capture support.
pub fn create_capturer() -> Result<PlatformCapture, CaptureError> {
    create_capturer_for_source(None)
}

/// Create a screen capturer targeting a specific source (monitor index).
///
/// `source_index` is the monitor index returned by `list_capture_sources`.
/// Pass `None` to capture the primary/default monitor.
#[allow(unused_variables, reason = "source_index unused on non-Linux platforms")]
pub fn create_capturer_for_source(
    source_index: Option<usize>,
) -> Result<PlatformCapture, CaptureError> {
    let inner: Box<dyn ErasedCapture> = create_platform_capturer(source_index)?;
    Ok(PlatformCapture { inner })
}

fn create_platform_capturer(
    source_index: Option<usize>,
) -> Result<Box<dyn ErasedCapture>, CaptureError> {
    #[cfg(target_os = "linux")]
    {
        let capturer = match source_index {
            Some(idx) => linux::PortalCapture::with_monitor(idx),
            None => linux::PortalCapture::new(),
        };
        Ok(Box::new(capturer))
    }

    #[cfg(target_os = "android")]
    {
        let _ = source_index;
        Ok(Box::new(android::AndroidCapture::new()))
    }

    #[cfg(target_os = "windows")]
    {
        let _ = source_index;
        Ok(Box::new(windows::WindowsCapture::new()))
    }

    #[cfg(target_os = "macos")]
    {
        let _ = source_index;
        Ok(Box::new(macos::MacCapture::new()))
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "windows",
        target_os = "macos"
    )))]
    {
        let _ = source_index;
        Err(CaptureError::Platform(
            "screen capture not implemented for this platform".into(),
        ))
    }
}

/// Information about an available capture source (monitor/screen).
#[derive(serde::Serialize, Clone)]
pub struct CaptureSourceInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
    pub thumbnail: String,
}

/// List available capture sources for the frontend picker.
pub fn list_capture_sources() -> Result<Vec<CaptureSourceInfo>, String> {
    #[cfg(target_os = "linux")]
    {
        linux::list_capture_sources()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(Vec::new())
    }
}

type ErasedFuture<'a> =
    Pin<Box<dyn Future<Output = Result<PlatformSession, CaptureError>> + Send + 'a>>;

/// Internal trait that erases the associated `Session` type so we can store
/// the capturer in a `Box<dyn>`.
trait ErasedCapture: Send + Sync + 'static {
    fn start_erased(&self, on_frame: FrameCallback) -> ErasedFuture<'_>;
}

impl<T: ScreenCapture> ErasedCapture for T {
    fn start_erased(&self, on_frame: FrameCallback) -> ErasedFuture<'_> {
        Box::pin(async move {
            let session = self.start(on_frame).await?;
            Ok(PlatformSession {
                _inner: Box::new(session),
            })
        })
    }
}
