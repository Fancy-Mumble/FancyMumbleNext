//! Rust-native screen capture and video streaming.
//!
//! On Linux, uses a `GStreamer` pipeline (`pipewiresrc` -> VP8 -> `WebM`) served
//! as `video/webm` over HTTP.  On other platforms, falls back to an
//! MJPEG-over-HTTP stream.

#[cfg(not(target_os = "linux"))]
pub mod capture;
#[cfg(not(target_os = "linux"))]
pub mod encoder;
#[cfg(target_os = "linux")]
mod gstreamer_pipeline;
#[cfg(not(target_os = "linux"))]
pub mod platform;
#[cfg(not(target_os = "linux"))]
pub mod stream_server;

#[cfg(not(target_os = "linux"))]
use std::sync::Arc;

#[cfg(not(target_os = "linux"))]
use capture::CaptureError;
#[cfg(not(target_os = "linux"))]
use encoder::{FrameEncoder, JpegEncoder};
#[cfg(not(target_os = "linux"))]
use platform::PlatformSession;
#[cfg(not(target_os = "linux"))]
use stream_server::{FrameSender, StreamServer};

#[cfg(not(target_os = "linux"))]
pub use platform::{CaptureSourceInfo, list_capture_sources};

// On Linux, the XDG portal picker handles source selection natively,
// so we provide a minimal inline definition instead of the full platform module.
#[cfg(target_os = "linux")]
#[derive(serde::Serialize, Clone)]
pub struct CaptureSourceInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
    pub thumbnail: String,
}

#[cfg(target_os = "linux")]
pub fn list_capture_sources() -> Result<Vec<CaptureSourceInfo>, String> {
    Ok(Vec::new())
}

// ===========================================================================
// Linux: GStreamer VP8/WebM pipeline
// ===========================================================================

/// Manages the full capture-encode-stream pipeline.
#[cfg(target_os = "linux")]
pub struct ScreenShareService {
    inner: gstreamer_pipeline::GstScreenCapture,
}

#[cfg(target_os = "linux")]
impl ScreenShareService {
    /// Start capturing and streaming via VP8/WebM.
    ///
    /// `source_index` is ignored on Linux (the XDG portal shows its own
    /// picker).  Returns the service handle and _`http://…/stream`_ URL for
    /// a `<video>` element.
    pub async fn start(_source_index: Option<usize>) -> Result<(Self, String), String> {
        let (capture, url) = gstreamer_pipeline::GstScreenCapture::start().await?;
        Ok((Self { inner: capture }, url))
    }

    /// URL the frontend should use.
    pub fn stream_url(&self) -> String {
        self.inner.stream_url()
    }

    /// Stop capturing and shut down all resources.
    pub async fn stop(self) {
        self.inner.stop().await;
    }
}

// ===========================================================================
// Non-Linux: MJPEG-over-HTTP fallback
// ===========================================================================

/// Manages the full capture-encode-stream pipeline.
#[cfg(not(target_os = "linux"))]
pub struct ScreenShareService {
    _session: PlatformSession,
    stream_server: StreamServer,
    _frame_sender: FrameSender,
}

#[cfg(not(target_os = "linux"))]
impl ScreenShareService {
    /// Start capturing a specific source and streaming.
    ///
    /// `source_index` is the monitor index from `list_capture_sources`.
    /// Pass `None` for the primary/default monitor.
    ///
    /// Returns the service handle and the URL the frontend should use to
    /// display the stream (`http://127.0.0.1:{port}/stream`).
    pub async fn start(source_index: Option<usize>) -> Result<(Self, String), String> {
        let capturer = platform::create_capturer_for_source(source_index)
            .map_err(|e| format!("create capturer: {e}"))?;

        let (server, frame_tx) = StreamServer::start()
            .await
            .map_err(|e| format!("start stream server: {e}"))?;

        let url = server.stream_url();
        tracing::info!("screen share: MJPEG URL = {url}");

        let encoder: Arc<dyn FrameEncoder> = Arc::new(JpegEncoder::new(60));
        let tx = frame_tx.clone();

        let callback = build_frame_callback(encoder, tx);

        tracing::info!("screen share: starting platform capture...");
        let session = capturer
            .start(callback)
            .await
            .map_err(capture_error_message)?;
        tracing::info!("screen share: capture session started, streaming frames");

        Ok((
            Self {
                _session: session,
                stream_server: server,
                _frame_sender: frame_tx,
            },
            url,
        ))
    }

    /// URL the frontend should connect to (`<img src="..."/>`).
    pub fn stream_url(&self) -> String {
        self.stream_server.stream_url()
    }

    /// Stop capturing and shut down the stream server.
    pub async fn stop(self) {
        drop(self._session);
        self.stream_server.stop().await;
    }
}

#[cfg(not(target_os = "linux"))]
fn build_frame_callback(encoder: Arc<dyn FrameEncoder>, tx: FrameSender) -> capture::FrameCallback {
    let (frame_tx, frame_rx) =
        std::sync::mpsc::sync_channel::<capture::CapturedFrame>(1);

    let _encoder_handle = std::thread::Builder::new()
        .name("mjpeg-encode".into())
        .spawn(move || encode_loop(encoder, frame_rx, tx))
        .ok();

    Box::new(move |frame| {
        let _ = frame_tx.try_send(frame);
    })
}

#[cfg(not(target_os = "linux"))]
fn encode_loop(
    encoder: Arc<dyn FrameEncoder>,
    rx: std::sync::mpsc::Receiver<capture::CapturedFrame>,
    tx: FrameSender,
) {
    let mut frame_count: u64 = 0;
    while let Ok(frame) = rx.recv() {
        match encoder.encode(&frame) {
            Ok(encoded) => {
                if frame_count == 0 {
                    tracing::info!(
                        "screen share: first frame received ({}x{}, {} bytes encoded)",
                        frame.width,
                        frame.height,
                        encoded.data.len(),
                    );
                }
                frame_count += 1;
                let _ = tx.send(Arc::new(encoded.data));
            }
            Err(e) => {
                tracing::warn!("frame encode error: {e}");
            }
        }
    }
    tracing::debug!("encoder thread exiting");
}

#[cfg(not(target_os = "linux"))]
fn capture_error_message(e: CaptureError) -> String {
    match e {
        CaptureError::PermissionDenied => "Screen share permission was denied".into(),
        CaptureError::NoScreen => "No screen available for capture".into(),
        CaptureError::Platform(msg) => format!("Screen capture failed: {msg}"),
    }
}

/// Screen share capabilities reported to the frontend.
#[derive(serde::Serialize, Clone)]
pub struct ScreenShareCapabilities {
    /// Whether native Rust-based capture is available on this platform.
    pub native_capture: bool,
    /// Whether browser `getDisplayMedia` WebRTC is expected to work.
    pub browser_webrtc: bool,
}

/// Query screen-sharing capabilities for the current platform.
pub fn capabilities() -> ScreenShareCapabilities {
    #[cfg(target_os = "linux")]
    let native_capture = true;
    #[cfg(not(target_os = "linux"))]
    let native_capture = platform::create_capturer().is_ok();

    let browser_webrtc = cfg!(target_os = "windows") || cfg!(target_os = "macos");

    ScreenShareCapabilities {
        native_capture,
        browser_webrtc,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, reason = "acceptable in test code")]
    use super::*;

    #[test]
    fn capabilities_returns_coherent_values() {
        let caps = capabilities();
        #[cfg(target_os = "linux")]
        {
            assert!(caps.native_capture, "GStreamer capture should work on Linux");
            assert!(!caps.browser_webrtc, "WebKitGTK has no WebRTC");
        }
    }

    #[test]
    fn capabilities_serializes_to_json() {
        let caps = capabilities();
        let json = serde_json::to_string(&caps).expect("serialize");
        assert!(json.contains("native_capture"));
        assert!(json.contains("browser_webrtc"));
    }
}
