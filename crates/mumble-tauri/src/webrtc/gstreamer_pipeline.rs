//! `GStreamer` MJPEG screen capture pipeline for Linux.
//!
//! Uses `GStreamer`'s hardware-accelerated `jpegenc` (libjpeg-turbo) to encode
//! screen frames captured via `pipewiresrc`, served as
//! `multipart/x-mixed-replace` over HTTP for zero-latency display in an
//! `<img>` tag.

use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::watch;


/// `GStreamer`-based screen capture that produces an MJPEG stream.
#[derive(Debug)]
pub struct GstScreenCapture {
    pipeline: gst::Pipeline,
    _fd: OwnedFd,
    port: u16,
    shutdown: Arc<AtomicBool>,
    server_task: tokio::task::JoinHandle<()>,
}

impl GstScreenCapture {
    /// Open the portal, build the `GStreamer` pipeline, and start streaming.
    ///
    /// Returns the capture handle and the `http://127.0.0.1:{port}/stream` URL
    /// that the frontend should load in an `<img>` element.
    pub async fn start() -> Result<(Self, String), String> {
        gst::init().map_err(|e| format!("GStreamer init failed: {e}"))?;

        let (fd, node_id) = open_screencast_portal().await?;
        tracing::info!("portal: PipeWire fd acquired, node_id={node_id}");

        let (pipeline, appsink) = build_pipeline(&fd, node_id)?;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("bind MJPEG server: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("local_addr: {e}"))?
            .port();

        let (frame_tx, frame_rx) = watch::channel::<Arc<Vec<u8>>>(Arc::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        start_appsink_reader(appsink, frame_tx, Arc::clone(&shutdown));

        set_pipeline_playing(&pipeline)?;
        tracing::info!("GStreamer MJPEG pipeline playing on port {port}");

        let server_task = tokio::spawn(serve_frames(
            listener,
            frame_rx,
            Arc::clone(&shutdown),
        ));

        let url = format!("http://127.0.0.1:{port}/stream");

        Ok((
            Self {
                pipeline,
                _fd: fd,
                port,
                shutdown,
                server_task,
            },
            url,
        ))
    }

    /// URL the frontend should connect to (`<img src="..."/>`).
    pub fn stream_url(&self) -> String {
        format!("http://127.0.0.1:{}/stream", self.port)
    }

    /// Stop capturing and shut down the pipeline and HTTP server.
    pub async fn stop(self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = self.pipeline.set_state(gst::State::Null);
        self.server_task.abort();
        let _ = self.server_task.await;
        tracing::info!("GStreamer screen capture stopped");
    }
}

// ---------------------------------------------------------------------------
// GStreamer pipeline construction
// ---------------------------------------------------------------------------

fn build_pipeline(
    fd: &OwnedFd,
    node_id: u32,
) -> Result<(gst::Pipeline, gst_app::AppSink), String> {
    let pipeline_str = "\
        pipewiresrc name=pwsrc do-timestamp=true \
        ! videoconvert \
        ! jpegenc quality=85 \
        ! appsink name=sink sync=false emit-signals=false";

    let element = gst::parse::launch(pipeline_str)
        .map_err(|e| format!("parse_launch: {e}"))?;

    let pipeline: gst::Pipeline = element
        .downcast()
        .map_err(|_| "parsed element is not a Pipeline".to_string())?;

    let pwsrc = pipeline
        .by_name("pwsrc")
        .ok_or_else(|| "pipewiresrc element not found".to_string())?;
    pwsrc.set_property("fd", fd.as_raw_fd());
    pwsrc.set_property("path", node_id.to_string());
    configure_keepalive(&pwsrc);

    let sink_element = pipeline
        .by_name("sink")
        .ok_or_else(|| "appsink element not found".to_string())?;
    let appsink: gst_app::AppSink = sink_element
        .downcast()
        .map_err(|_| "element 'sink' is not an AppSink".to_string())?;

    Ok((pipeline, appsink))
}

fn configure_keepalive(pwsrc: &gst::Element) {
    let keepalive_ms: i32 = 100;
    if pwsrc.find_property("keepalive-time").is_some() {
        pwsrc.set_property("keepalive-time", keepalive_ms);
        tracing::info!("pipewiresrc keepalive-time set to {keepalive_ms} ms");
    } else {
        tracing::warn!(
            "pipewiresrc has no keepalive-time property; \
             screen may freeze when idle"
        );
    }
}

fn set_pipeline_playing(pipeline: &gst::Pipeline) -> Result<(), String> {
    if let Err(e) = pipeline.set_state(gst::State::Playing) {
        let bus_error = pipeline
            .bus()
            .and_then(|bus| {
                bus.timed_pop_filtered(
                    gst::ClockTime::from_mseconds(500),
                    &[gst::MessageType::Error],
                )
            })
            .and_then(|msg| match msg.view() {
                gst::MessageView::Error(err) => {
                    let dbg = err
                        .debug()
                        .map(|d| format!(" ({d})"))
                        .unwrap_or_default();
                    Some(format!("{}{dbg}", err.error()))
                }
                _ => None,
            });

        let detail = bus_error.unwrap_or_else(|| e.to_string());

        return Err(format!("set pipeline Playing: {detail}"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Appsink reader thread (pulls JPEG frames)
// ---------------------------------------------------------------------------

fn start_appsink_reader(
    appsink: gst_app::AppSink,
    frame_tx: watch::Sender<Arc<Vec<u8>>>,
    shutdown: Arc<AtomicBool>,
) {
    let _ = std::thread::Builder::new()
        .name("gst-mjpeg-pull".into())
        .spawn(move || {
            tracing::debug!("appsink reader started");
            let mut total_bytes: u64 = 0;
            let mut frame_count: u64 = 0;
            let mut last_log = std::time::Instant::now();
            let mut frames_since_log: u64 = 0;

            loop {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }

                let Ok(sample) = appsink.pull_sample() else {
                    tracing::debug!("appsink EOS or flushing, reader exiting");
                    break;
                };

                let Some(gst_buffer) = sample.buffer() else {
                    continue;
                };

                let Ok(map) = gst_buffer.map_readable() else {
                    continue;
                };

                let jpeg = map.as_slice();
                if jpeg.is_empty() {
                    continue;
                }

                total_bytes += jpeg.len() as u64;
                frame_count += 1;
                frames_since_log += 1;

                if frame_count == 1 {
                    tracing::info!(
                        "GStreamer: first JPEG frame ({} bytes)",
                        jpeg.len()
                    );
                }

                let now = std::time::Instant::now();
                if now.duration_since(last_log).as_secs() >= 2 {
                    let elapsed = now.duration_since(last_log).as_secs_f32();
                    let fps = frames_since_log as f32 / elapsed;
                    tracing::debug!(
                        "appsink: {fps:.1} fps, {frames_since_log} frames in {elapsed:.1}s"
                    );
                    frames_since_log = 0;
                    last_log = now;
                }

                let _ = frame_tx.send(Arc::new(jpeg.to_vec()));
            }

            tracing::info!(
                "appsink reader done: {frame_count} frames, {total_bytes} bytes total"
            );
        })
        .ok();
}

// ---------------------------------------------------------------------------
// Single-frame HTTP server (fetch-polling from frontend)
// ---------------------------------------------------------------------------

async fn serve_frames(
    listener: TcpListener,
    frame_rx: watch::Receiver<Arc<Vec<u8>>>,
    shutdown: Arc<AtomicBool>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let mut rx = frame_rx.clone();
                        let stop = Arc::clone(&shutdown);
                        drop(tokio::spawn(async move {
                            let _ = serve_single_frame(stream, &mut rx, &stop).await;
                        }));
                    }
                    Err(e) => {
                        tracing::warn!("frame server accept error: {e}");
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }
}

async fn serve_single_frame(
    mut stream: tokio::net::TcpStream,
    frame_rx: &mut watch::Receiver<Arc<Vec<u8>>>,
    shutdown: &Arc<AtomicBool>,
) -> std::io::Result<()> {
    skip_http_request(&mut stream).await?;
    let _ = stream.set_nodelay(true);

    let frame = loop {
        if shutdown.load(Ordering::Relaxed) {
            return Ok(());
        }
        let current = frame_rx.borrow_and_update().clone();
        if !current.is_empty() {
            break current;
        }
        match tokio::time::timeout(
            std::time::Duration::from_millis(500),
            frame_rx.changed(),
        )
        .await
        {
            Ok(Ok(())) => continue,
            _ => return Ok(()),
        }
    };

    let header = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: image/jpeg\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-cache, no-store, must-revalidate\r\n\
         Pragma: no-cache\r\n\
         Expires: 0\r\n\
         Connection: close\r\n\
         Access-Control-Allow-Origin: *\r\n\
         \r\n",
        frame.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&frame).await?;
    Ok(())
}

async fn skip_http_request(stream: &mut tokio::net::TcpStream) -> std::io::Result<()> {
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 4096];
    let _ = stream.read(&mut buf).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// XDG ScreenCast portal via ashpd
// ---------------------------------------------------------------------------

async fn open_screencast_portal() -> Result<(OwnedFd, u32), String> {
    use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
    use ashpd::desktop::PersistMode;

    let proxy = Screencast::new()
        .await
        .map_err(|e| format!("screencast proxy: {e}"))?;

    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(|e| format!("create session: {e}"))?;

    let _request = proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Embedded)
                .set_sources(SourceType::Monitor | SourceType::Window)
                .set_multiple(false)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await
        .map_err(|e| format!("select sources: {e}"))?;

    let response = proxy
        .start(&session, None, Default::default())
        .await
        .map_err(|e| format!("start screencast: {e}"))?
        .response()
        .map_err(|e| format!("screencast response: {e}"))?;

    let streams = response.streams();
    let stream = streams
        .first()
        .ok_or_else(|| "no screen selected".to_string())?;

    let node_id = stream.pipe_wire_node_id();
    tracing::info!(
        "portal: selected node_id={node_id}, size={:?}",
        stream.size()
    );

    let fd = proxy
        .open_pipe_wire_remote(&session, Default::default())
        .await
        .map_err(|e| format!("open PipeWire remote: {e}"))?;

    Ok((fd, node_id))
}
