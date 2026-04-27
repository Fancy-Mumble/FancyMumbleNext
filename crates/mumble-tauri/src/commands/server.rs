//! Server-info & ping commands.

use crate::state::{AppState, ServerConfig, ServerInfo};

/// Result of a server ping attempt.
#[derive(serde::Serialize, Clone)]
pub(crate) struct PingResult {
    online: bool,
    /// Round-trip time in milliseconds (None when offline).
    latency_ms: Option<u32>,
    /// Number of users currently connected (from UDP ping, None if unavailable).
    user_count: Option<u32>,
    /// Maximum number of users allowed (from UDP ping, None if unavailable).
    max_user_count: Option<u32>,
    /// Server version string (e.g. "1.5.634"), None if unavailable.
    server_version: Option<String>,
}

#[tauri::command]
pub(crate) fn get_server_config(state: tauri::State<'_, AppState>) -> ServerConfig {
    state.server_config()
}

/// Get aggregated server info (version, host, users, codec, etc.).
#[tauri::command]
pub(crate) fn get_server_info(state: tauri::State<'_, AppState>) -> ServerInfo {
    state.server_info()
}

/// Get the server welcome text (HTML) received during handshake.
#[tauri::command]
pub(crate) fn get_welcome_text(state: tauri::State<'_, AppState>) -> Option<String> {
    state.welcome_text()
}

/// Ping a Mumble server to measure latency and retrieve server info.
///
/// Performs two concurrent probes:
/// 1. **TCP connect** - measures round-trip latency.
/// 2. **UDP ping** - sends a Mumble protocol ping with
///    `request_extended_information` to retrieve user count and max users.
#[tauri::command]
pub(crate) async fn ping_server(host: String, port: u16) -> PingResult {
    use std::time::Instant;
    use tokio::net::TcpStream;
    use tokio::time::{timeout, Duration};

    let addr = format!("{host}:{port}");
    let start = Instant::now();

    // TCP latency probe
    let tcp_online = timeout(Duration::from_secs(4), TcpStream::connect(&addr)).await;
    let (online, latency_ms) = match tcp_online {
        Ok(Ok(_stream)) => (true, Some(start.elapsed().as_millis() as u32)),
        _ => (false, None),
    };

    // UDP ping for user count + version (best-effort, does not affect online status)
    let (user_count, max_user_count, server_version) =
        udp_ping_server_info(&addr).await.unwrap_or((None, None, None));

    PingResult {
        online,
        latency_ms,
        user_count,
        max_user_count,
        server_version,
    }
}

/// Decode a Mumble `version_v2` u64 into a human-readable string.
///
/// Encoding: `(major << 48) | (minor << 32) | (patch << 16)`.
fn format_version_v2(v: u64) -> Option<String> {
    if v == 0 {
        return None;
    }
    let major = (v >> 48) & 0xFFFF;
    let minor = (v >> 32) & 0xFFFF;
    let patch = (v >> 16) & 0xFFFF;
    Some(format!("{major}.{minor}.{patch}"))
}

/// Decode a legacy Mumble version u32 into a human-readable string.
///
/// Encoding: `(major << 16) | (minor << 8) | patch`.
fn format_version_legacy(v: u32) -> Option<String> {
    if v == 0 {
        return None;
    }
    let major = (v >> 16) & 0xFF;
    let minor = (v >> 8) & 0xFF;
    let patch = v & 0xFF;
    Some(format!("{major}.{minor}.{patch}"))
}

/// Send a Mumble UDP ping to retrieve extended server information.
///
/// Returns `(user_count, max_user_count, server_version)` on success.
/// Tries the protobuf format first; falls back to the legacy 12-byte
/// format if the server doesn't respond to protobuf within the timeout.
async fn udp_ping_server_info(addr: &str) -> Result<(Option<u32>, Option<u32>, Option<String>), ()> {
    use prost::Message;
    use tokio::net::UdpSocket;
    use tokio::time::{timeout, Duration};

    let sock = UdpSocket::bind("0.0.0.0:0").await.map_err(|_| ())?;
    sock.connect(addr).await.map_err(|_| ())?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    // Try protobuf ping first (Mumble 1.5+ servers)
    let ping = mumble_protocol::proto::mumble_udp::Ping {
        timestamp: ts,
        request_extended_information: true,
        ..Default::default()
    };
    let mut buf = Vec::with_capacity(16);
    buf.push(0x20); // UDP Ping type marker
    ping.encode(&mut buf).map_err(|_| ())?;
    let _sent = sock.send(&buf).await.map_err(|_| ())?;

    let mut recv_buf = [0u8; 128];
    if let Ok(Ok(n)) = timeout(Duration::from_secs(2), sock.recv(&mut recv_buf)).await {
        if n > 1 && recv_buf[0] == 0x20 {
            // Protobuf response
            if let Ok(resp) =
                mumble_protocol::proto::mumble_udp::Ping::decode(&recv_buf[1..n])
            {
                if resp.user_count > 0 || resp.max_user_count > 0 || resp.server_version_v2 > 0 {
                    let version = format_version_v2(resp.server_version_v2);
                    return Ok((Some(resp.user_count), Some(resp.max_user_count), version));
                }
            }
        }
        // Legacy 24-byte response: 6 x u32 big-endian
        // [version, ts_hi, ts_lo, users, max_users, bandwidth]
        if n >= 24 {
            let ver = u32::from_be_bytes([recv_buf[0], recv_buf[1], recv_buf[2], recv_buf[3]]);
            let users = u32::from_be_bytes([recv_buf[12], recv_buf[13], recv_buf[14], recv_buf[15]]);
            let max_users = u32::from_be_bytes([recv_buf[16], recv_buf[17], recv_buf[18], recv_buf[19]]);
            return Ok((Some(users), Some(max_users), format_version_legacy(ver)));
        }
    }

    // Fallback: send legacy 12-byte ping (4 zero bytes + 8-byte timestamp)
    let mut legacy = [0u8; 12];
    legacy[4..12].copy_from_slice(&ts.to_be_bytes());
    let _ = sock.send(&legacy).await;

    if let Ok(Ok(n)) = timeout(Duration::from_secs(2), sock.recv(&mut recv_buf)).await {
        if n >= 24 {
            let ver = u32::from_be_bytes([recv_buf[0], recv_buf[1], recv_buf[2], recv_buf[3]]);
            let users = u32::from_be_bytes([recv_buf[12], recv_buf[13], recv_buf[14], recv_buf[15]]);
            let max_users = u32::from_be_bytes([recv_buf[16], recv_buf[17], recv_buf[18], recv_buf[19]]);
            return Ok((Some(users), Some(max_users), format_version_legacy(ver)));
        }
    }

    Ok((None, None, None))
}
