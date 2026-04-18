//! Android screen capture via `MediaProjection` API.
//!
//! On Android the capture requires a foreground service with
//! `FOREGROUND_SERVICE_MEDIA_PROJECTION` type and user consent via
//! `MediaProjectionManager.createScreenCaptureIntent()`.
//!
//! The actual JNI bridge calls are stubbed for now.  They need a running
//! Android activity context to obtain the projection token.

use crate::webrtc::capture::{
    CaptureError, CaptureSession, CapturedFrame, FrameCallback, ScreenCapture,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Android `MediaProjection`-based screen capture.
pub struct AndroidCapture {
    _private: (),
}

impl AndroidCapture {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

/// Active Android capture session.  Dropping stops the virtual display.
pub struct AndroidSession {
    stop: Arc<AtomicBool>,
}

impl CaptureSession for AndroidSession {}

impl Drop for AndroidSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl ScreenCapture for AndroidCapture {
    type Session = AndroidSession;

    async fn start(&self, _on_frame: FrameCallback) -> Result<AndroidSession, CaptureError> {
        // TODO: Implement via JNI bridge:
        //   1. Get Activity from Tauri's AndroidPlugin handle
        //   2. Launch MediaProjectionManager.createScreenCaptureIntent()
        //   3. Create VirtualDisplay with an ImageReader surface
        //   4. Deliver frames via on_frame callback
        Err(CaptureError::Platform(
            "Android MediaProjection capture not yet implemented".into(),
        ))
    }
}
