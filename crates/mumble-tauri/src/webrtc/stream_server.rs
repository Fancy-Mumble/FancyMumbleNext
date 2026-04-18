//! Local MJPEG-over-HTTP server for video playback in `<img>` / `<video>` tags.
//!
//! Serves an infinite `multipart/x-mixed-replace` stream of JPEG frames.
//! The frontend connects with `<img src="http://127.0.0.1:{port}/stream">`.

use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::watch;

/// A running MJPEG stream server.
pub struct StreamServer {
    port: u16,
    shutdown: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

/// Sender half: push new JPEG frames to all connected viewers.
pub type FrameSender = watch::Sender<Arc<Vec<u8>>>;

/// Receiver half: used internally by the server to read the latest frame.
pub type FrameReceiver = watch::Receiver<Arc<Vec<u8>>>;

impl StreamServer {
    /// Start the server on a random available port.
    ///
    /// Returns the server handle and a [`FrameSender`] to push new JPEG
    /// frames.  Push frames with `sender.send(Arc::new(jpeg_bytes))`.
    pub async fn start() -> std::io::Result<(Self, FrameSender)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();

        let empty_frame: Arc<Vec<u8>> = Arc::new(Vec::new());
        let (frame_tx, frame_rx) = watch::channel(empty_frame);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let task = tokio::spawn(run_server(listener, frame_rx, shutdown_rx));

        tracing::info!("MJPEG stream server listening on 127.0.0.1:{port}");

        Ok((
            Self {
                port,
                shutdown: shutdown_tx,
                task,
            },
            frame_tx,
        ))
    }

    /// URL the frontend should use to connect.
    pub fn stream_url(&self) -> String {
        format!("http://127.0.0.1:{}/stream", self.port)
    }

    /// Gracefully shut down the server.
    pub async fn stop(self) {
        let _ = self.shutdown.send(true);
        let _ = self.task.await;
    }
}

const BOUNDARY: &str = "fancy-mjpeg-boundary";

async fn run_server(
    listener: TcpListener,
    frame_rx: FrameReceiver,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        tracing::debug!("MJPEG client connected from {addr}");
                        let rx = frame_rx.clone();
                        let mut shutdown = shutdown_rx.clone();
                        let _client_task = tokio::spawn(async move {
                            if let Err(e) = handle_client(stream, rx, &mut shutdown).await {
                                tracing::debug!("MJPEG client {addr} disconnected: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!("MJPEG accept error: {e}");
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    tracing::info!("MJPEG stream server shutting down");
                    break;
                }
            }
        }
    }
}

async fn handle_client(
    mut stream: tokio::net::TcpStream,
    mut frame_rx: FrameReceiver,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> std::io::Result<()> {
    skip_http_request(&mut stream).await?;

    let header = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: multipart/x-mixed-replace; boundary={BOUNDARY}\r\n\
         Cache-Control: no-cache, no-store\r\n\
         Connection: close\r\n\
         Access-Control-Allow-Origin: *\r\n\
         \r\n"
    );
    stream.write_all(header.as_bytes()).await?;

    loop {
        tokio::select! {
            result = frame_rx.changed() => {
                if result.is_err() {
                    break;
                }
                let frame = frame_rx.borrow_and_update().clone();
                if frame.is_empty() {
                    continue;
                }
                write_mjpeg_part(&mut stream, &frame).await?;
            }
            _ = shutdown_rx.changed() => {
                break;
            }
        }
    }

    Ok(())
}

async fn write_mjpeg_part(
    stream: &mut tokio::net::TcpStream,
    jpeg_data: &[u8],
) -> std::io::Result<()> {
    let part = format!(
        "--{BOUNDARY}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
        jpeg_data.len()
    );
    stream.write_all(part.as_bytes()).await?;
    stream.write_all(jpeg_data).await?;
    stream.write_all(b"\r\n").await?;
    stream.flush().await
}

async fn skip_http_request(stream: &mut tokio::net::TcpStream) -> std::io::Result<()> {
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 4096];
    let _ = stream.read(&mut buf).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[tokio::test]
    async fn server_starts_and_returns_url() {
        let (server, _tx) = StreamServer::start().await.unwrap();
        let url = server.stream_url();
        assert!(url.starts_with("http://127.0.0.1:"));
        assert!(url.ends_with("/stream"));
        server.stop().await;
    }

    #[tokio::test]
    async fn server_streams_mjpeg_frames() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (server, tx) = StreamServer::start().await.unwrap();
        let url = server.stream_url();
        let port: u16 = url
            .strip_prefix("http://127.0.0.1:")
            .unwrap()
            .strip_suffix("/stream")
            .unwrap()
            .parse()
            .unwrap();

        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        client
            .write_all(b"GET /stream HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();

        // Send a test frame
        let fake_jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        tx.send(Arc::new(fake_jpeg.clone())).unwrap();

        // Read response - should contain the MJPEG boundary and our data
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let mut buf = vec![0u8; 4096];
        let n = client.read(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);

        assert!(
            response.contains("multipart/x-mixed-replace"),
            "missing multipart content type"
        );
        assert!(
            response.contains(BOUNDARY),
            "missing MJPEG boundary"
        );

        server.stop().await;
    }

    #[tokio::test]
    async fn server_stop_is_graceful() {
        let (server, _tx) = StreamServer::start().await.unwrap();
        // Stopping should not panic or hang
        server.stop().await;
    }
}
