//! Tauri application entry point with Mumble backend commands.
//!
// All public command functions receive `tauri::State` by value, which is
// required by the `#[tauri::command]` macro - suppress the lint crate-wide.
#![allow(clippy::needless_pass_by_value, reason = "tauri::command requires State<T> to be taken by value")]
// This is an application crate; pub items inside private modules are
// intentional (proc-macro visibility, Tauri command system, internal APIs).
#![allow(unreachable_pub, reason = "application crate: pub items in private modules are intentional for Tauri command system")]

mod audio;
#[cfg(target_os = "linux")]
mod linux_desktop;
mod state;
#[cfg(not(target_os = "android"))]
mod tray;
#[cfg(target_os = "android")]
mod connection_service;
#[cfg(target_os = "android")]
mod fcm_service;

use state::{
    AppState, AudioDevice, AudioSettings, ChannelEntry, ChatMessage, ConnectionStatus,
    DebugStats, GroupChat, PhotoEntry, SearchResult, ServerConfig, ServerInfo, UserEntry,
    VoiceState,
};
use std::collections::HashMap;
use std::sync::OnceLock;
use tauri::Manager;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload;

/// Global handle for reloading the tracing filter at runtime.
static LOG_RELOAD_HANDLE: OnceLock<reload::Handle<EnvFilter, tracing_subscriber::Registry>> =
    OnceLock::new();

// --- Windows system clock detection ------------------------------

// GetLocaleInfoW from kernel32 - used to read the user's regional settings.
#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
extern "system" {
    fn GetLocaleInfoW(locale: u32, lctype: u32, lp_lc_data: *mut u16, cch_data: i32) -> i32;
}

/// Returns true when the Windows regional settings use a 24-hour clock.
///
/// Reads `LOCALE_ITIME` ("0" = 12-hour, "1" = 24-hour) via `GetLocaleInfoW`.
#[cfg(target_os = "windows")]
#[allow(unsafe_code, reason = "GetLocaleInfoW is a safe Windows API call wrapped with an unsafe extern block")]
fn system_uses_24h() -> Option<bool> {
    const LOCALE_USER_DEFAULT: u32 = 0x0400;
    const LOCALE_ITIME: u32 = 0x0019;
    let mut buf = [0u16; 4];
    let len = unsafe { GetLocaleInfoW(LOCALE_USER_DEFAULT, LOCALE_ITIME, buf.as_mut_ptr(), 4) };
    if len <= 0 {
        return None;
    }
    // Docs say "0" = 12-hour, "1" = 24-hour, but some locales (e.g. de-DE)
    // return "2". Only "0" is exclusively 12-hour; treat anything else as 24h.
    Some(
        buf[..(len as usize).saturating_sub(1)]
            .first()
            .copied()
            .map(|c| c != b'0' as u16)
            .unwrap_or(false),
    )
}

/// On non-Windows, `WebView` Intl resolution is reliable so we return `None`
/// and let the frontend probe it directly.
#[cfg(not(target_os = "windows"))]
fn system_uses_24h() -> Option<bool> {
    None
}

/// Returns the OS-detected clock format for the "auto" time setting.
///
/// On Windows, `WebView2` (Chromium) derives the hour cycle from the ICU/CLDR
/// language-tag default (e.g. `en-US` is always 12h) and ignores the Windows
/// Region time-format setting, so the backend must read it directly.
/// Returns `None` on non-Windows platforms where the `WebView` Intl API
/// already honours the system locale.
#[tauri::command]
fn get_system_clock_format() -> Option<&'static str> {
    system_uses_24h().map(|h24| if h24 { "24h" } else { "12h" })
}

/// Result of a server ping attempt.
#[derive(serde::Serialize, Clone)]
struct PingResult {
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

// --- Badge overlay icon (Windows) ---------------------------------

/// Render a small 16x16 RGBA image with a red circle and white digit(s).
///
/// Used on Windows where `set_badge_count` is unsupported and the overlay
/// icon API must be used instead.
#[cfg(target_os = "windows")]
fn render_badge_icon(count: u32) -> Vec<u8> {
    const SIZE: usize = 16;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];

    // Draw a filled red circle (center 7.5, 7.5, radius 7.5).
    let cx = 7.5_f64;
    let cy = 7.5_f64;
    let r = 7.5_f64;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            if dx * dx + dy * dy <= r * r {
                let i = (y * SIZE + x) * 4;
                rgba[i] = 220;     // R
                rgba[i + 1] = 38;  // G
                rgba[i + 2] = 38;  // B
                rgba[i + 3] = 255; // A
            }
        }
    }

    // Stamp a white digit/text into the circle using a tiny 3x5 font.
    let label = if count > 99 {
        "!".to_string()
    } else {
        count.to_string()
    };
    stamp_text(&mut rgba, SIZE, &label);
    rgba
}

/// Tiny 3x5 pixel font for digits 0-9 and "!".
/// Each glyph is stored as 5 rows of 3 bits (MSB = left pixel).
#[cfg(target_os = "windows")]
fn glyph(ch: char) -> [u8; 5] {
    match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '!' => [0b010, 0b010, 0b010, 0b000, 0b010],
        _   => [0b000; 5],
    }
}

/// Stamp a short text string (1-2 chars) centered in a 16x16 RGBA buffer.
#[cfg(target_os = "windows")]
fn stamp_text(rgba: &mut [u8], size: usize, text: &str) {
    let chars: Vec<char> = text.chars().collect();
    let glyph_w = 3;
    let glyph_h = 5;
    let spacing = 1;
    let total_w = chars.len() * glyph_w + chars.len().saturating_sub(1) * spacing;
    let start_x = (size.saturating_sub(total_w)) / 2;
    let start_y = (size.saturating_sub(glyph_h)) / 2;

    for (ci, &ch) in chars.iter().enumerate() {
        let g = glyph(ch);
        let ox = start_x + ci * (glyph_w + spacing);
        for (row, &bits) in g.iter().enumerate() {
            for col in 0..glyph_w {
                if bits & (1 << (glyph_w - 1 - col)) != 0 {
                    set_pixel(rgba, size, ox + col, start_y + row);
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn set_pixel(rgba: &mut [u8], size: usize, px: usize, py: usize) {
    if px < size && py < size {
        let i = (py * size + px) * 4;
        rgba[i] = 255;
        rgba[i + 1] = 255;
        rgba[i + 2] = 255;
        rgba[i + 3] = 255;
    }
}

// --- Tauri commands -----------------------------------------------

#[tauri::command]
async fn connect(
    state: tauri::State<'_, AppState>,
    host: String,
    port: u16,
    username: String,
    cert_label: Option<String>,
    password: Option<String>,
) -> Result<(), String> {
    state.connect(host, port, username, cert_label, password).await
}

/// Generate a self-signed TLS client certificate for an identity label.
/// Each identity gets its own folder under `{app_data}/identities/{label}/`
/// containing both the TLS cert and the pchat seed.
/// Does nothing if the certificate already exists.
#[tauri::command]
async fn generate_certificate(
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    state::pchat::IdentityStore::new(data_dir).generate_cert(&label)
}

/// List the labels of all identities stored in `{app_data_dir}/identities/`.
#[tauri::command]
async fn list_certificates(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    Ok(state::pchat::IdentityStore::new(data_dir).list_labels())
}

/// Delete an identity (TLS cert + pchat seed) by label.
#[tauri::command]
async fn delete_certificate(
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    state::pchat::IdentityStore::new(data_dir).delete(&label)
}

/// Export an identity to a user-chosen file via the native save dialog.
#[tauri::command]
async fn export_certificate(
    app: tauri::AppHandle,
    label: String,
    dest_path: String,
) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    state::pchat::IdentityStore::new(data_dir).export(&label, std::path::Path::new(&dest_path))
}

/// Import an identity from a user-chosen file via the native open dialog.
/// Returns the label of the imported identity.
#[tauri::command]
async fn import_certificate(
    app: tauri::AppHandle,
    src_path: String,
) -> Result<String, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    state::pchat::IdentityStore::new(data_dir).import(std::path::Path::new(&src_path))
}

#[tauri::command]
async fn disconnect(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.disconnect().await
}

#[tauri::command]
fn get_status(state: tauri::State<'_, AppState>) -> ConnectionStatus {
    state.status()
}

#[tauri::command]
fn get_channels(state: tauri::State<'_, AppState>) -> Vec<ChannelEntry> {
    state.channels()
}

#[tauri::command]
fn get_users(state: tauri::State<'_, AppState>) -> Vec<UserEntry> {
    state.users()
}

#[tauri::command]
fn super_search(
    state: tauri::State<'_, AppState>,
    query: String,
    filter: Option<state::types::SearchFilter>,
    channel_id: Option<u32>,
) -> Vec<SearchResult> {
    state.super_search(&query, filter.unwrap_or(state::types::SearchFilter::All), channel_id)
}

#[tauri::command]
fn get_photos(
    state: tauri::State<'_, AppState>,
    offset: usize,
    limit: usize,
) -> Vec<PhotoEntry> {
    state.get_photos(offset, limit)
}

#[tauri::command]
fn get_messages(state: tauri::State<'_, AppState>, channel_id: u32) -> Vec<ChatMessage> {
    state.messages(channel_id)
}

#[tauri::command]
async fn send_message(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    body: String,
) -> Result<(), String> {
    state.send_message(channel_id, body).await
}

#[tauri::command]
async fn select_channel(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    state.select_channel(channel_id).await
}

#[tauri::command]
async fn join_channel(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    state.join_channel(channel_id).await
}

#[tauri::command]
fn get_current_channel(state: tauri::State<'_, AppState>) -> Option<u32> {
    state.current_channel()
}

#[tauri::command]
async fn toggle_listen(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<bool, String> {
    state.toggle_listen(channel_id).await
}

#[tauri::command]
fn get_listened_channels(state: tauri::State<'_, AppState>) -> Vec<u32> {
    state.listened_channels()
}

#[tauri::command]
fn get_push_subscribed_channels(state: tauri::State<'_, AppState>) -> Vec<u32> {
    state.push_subscribed_channels()
}

#[tauri::command]
fn get_unread_counts(state: tauri::State<'_, AppState>) -> HashMap<u32, u32> {
    state.unread_counts()
}

#[tauri::command]
fn mark_channel_read(state: tauri::State<'_, AppState>, channel_id: u32) {
    state.mark_read(channel_id);
}

#[tauri::command]
fn get_server_config(state: tauri::State<'_, AppState>) -> ServerConfig {
    state.server_config()
}

/// Get aggregated server info (version, host, users, codec, etc.).
#[tauri::command]
fn get_server_info(state: tauri::State<'_, AppState>) -> ServerInfo {
    state.server_info()
}

/// Get the server welcome text (HTML) received during handshake.
#[tauri::command]
fn get_welcome_text(state: tauri::State<'_, AppState>) -> Option<String> {
    state.welcome_text()
}

/// Update a channel on the server.
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "Tauri command mirrors the full channel update parameter surface")]
async fn update_channel(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    name: Option<String>,
    description: Option<String>,
    position: Option<i32>,
    temporary: Option<bool>,
    max_users: Option<u32>,
    pchat_protocol: Option<String>,
    pchat_max_history: Option<u32>,
    pchat_retention_days: Option<u32>,
) -> Result<(), String> {
    state
        .update_channel(
            channel_id,
            name,
            description,
            position,
            temporary,
            max_users,
            pchat_protocol,
            pchat_max_history,
            pchat_retention_days,
        )
        .await
}

/// Delete a channel on the server.
#[tauri::command]
async fn delete_channel(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    state.delete_channel(channel_id).await
}

/// Create a new sub-channel on the server.
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "Tauri command mirrors the full channel creation parameter surface")]
async fn create_channel(
    state: tauri::State<'_, AppState>,
    parent_id: u32,
    name: String,
    description: Option<String>,
    position: Option<i32>,
    temporary: Option<bool>,
    max_users: Option<u32>,
    pchat_protocol: Option<String>,
    pchat_max_history: Option<u32>,
    pchat_retention_days: Option<u32>,
) -> Result<(), String> {
    state
        .create_channel(
            parent_id,
            name,
            description,
            position,
            temporary,
            max_users,
            pchat_protocol,
            pchat_max_history,
            pchat_retention_days,
        )
        .await
}

/// Ping a Mumble server to measure latency and retrieve server info.
///
/// Performs two concurrent probes:
/// 1. **TCP connect** - measures round-trip latency.
/// 2. **UDP ping** - sends a Mumble protocol ping with
///    `request_extended_information` to retrieve user count and max users.
#[tauri::command]
async fn ping_server(host: String, port: u16) -> PingResult {
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

// --- Audio device commands ----------------------------------------

/// A public Mumble server from the official directory.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
struct PublicServer {
    name: String,
    country: String,
    country_code: String,
    ip: String,
    port: u16,
    region: String,
    url: String,
}

/// XML wrapper: `<servers><server .../> ...</servers>`
#[derive(serde::Deserialize, Debug)]
struct ServersXml {
    #[serde(rename = "server", default)]
    server: Vec<ServerXml>,
}

/// A single `<server ... />` element with attributes.
#[derive(serde::Deserialize, Debug)]
struct ServerXml {
    #[serde(rename = "@name", default)]
    name: String,
    #[serde(rename = "@country", default)]
    country: String,
    #[serde(rename = "@country_code", default)]
    country_code: String,
    #[serde(rename = "@ip", default)]
    ip: String,
    #[serde(rename = "@port", default = "default_port")]
    port: u16,
    #[serde(rename = "@region", default)]
    region: String,
    #[serde(rename = "@url", default)]
    url: String,
}

fn default_port() -> u16 {
    64738
}

/// Parse the Mumble public server list XML into a vec of [`PublicServer`].
fn parse_public_server_xml(xml: &str) -> Result<Vec<PublicServer>, String> {
    let parsed: ServersXml =
        quick_xml::de::from_str(xml).map_err(|e| format!("XML parse error: {e}"))?;

    Ok(parsed
        .server
        .into_iter()
        .map(|s| PublicServer {
            name: s.name,
            country: s.country,
            country_code: s.country_code,
            ip: s.ip,
            port: s.port,
            region: s.region,
            url: s.url,
        })
        .collect())
}

/// Fetch the official Mumble public server list.
///
/// The list is served as XML from `https://publist.mumble.info/v1/list`.
#[tauri::command]
async fn fetch_public_servers() -> Result<Vec<PublicServer>, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) FancyMumble/1.0 Safari/537.36")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let response = client
        .get("https://publist.mumble.info/v1/list")
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch public server list: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("Server returned HTTP {status}"));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    tracing::debug!("Public server list: {} bytes received", body.len());

    let servers = parse_public_server_xml(&body)?;

    tracing::debug!("Fetched {} public servers", servers.len());
    Ok(servers)
}

/// List available audio input devices (microphones).
/// Only available on desktop (cpal is not supported on Android).
#[cfg(not(target_os = "android"))]
#[tauri::command]
fn get_audio_devices() -> Vec<AudioDevice> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| {
            d.description()
                .ok()
                .map(|desc| desc.name().to_string())
        });

    host.input_devices()
        .map(|devices| {
            devices
                .filter_map(|d| {
                    let name = d
                        .description()
                        .ok()
                        .map(|desc| desc.name().to_string())?;
                    Some(AudioDevice {
                        name: name.clone(),
                        is_default: default_name.as_deref() == Some(&name),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Stub: on Android, return an empty device list.
#[cfg(target_os = "android")]
#[tauri::command]
fn get_audio_devices() -> Vec<AudioDevice> {
    Vec::new()
}

/// List available audio output devices (speakers/headphones).
/// Only available on desktop (cpal is not supported on Android).
#[cfg(not(target_os = "android"))]
#[tauri::command]
fn get_output_devices() -> Vec<AudioDevice> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|d| {
            d.description()
                .ok()
                .map(|desc| desc.name().to_string())
        });

    host.output_devices()
        .map(|devices| {
            devices
                .filter_map(|d| {
                    let name = d
                        .description()
                        .ok()
                        .map(|desc| desc.name().to_string())?;
                    Some(AudioDevice {
                        name: name.clone(),
                        is_default: default_name.as_deref() == Some(&name),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Stub: on Android, return an empty device list.
#[cfg(target_os = "android")]
#[tauri::command]
fn get_output_devices() -> Vec<AudioDevice> {
    Vec::new()
}

/// Get current audio settings.
#[tauri::command]
fn get_audio_settings(state: tauri::State<'_, AppState>) -> AudioSettings {
    state.audio_settings()
}

/// Update audio settings.
///
/// If any pipeline-relevant setting changes while voice is active, the
/// capture/playback pipelines are automatically restarted as needed.
#[tauri::command]
async fn set_audio_settings(
    state: tauri::State<'_, AppState>,
    settings: AudioSettings,
) -> Result<(), String> {
    let force_tcp = settings.force_tcp_audio;
    let (needs_outbound, needs_inbound, force_tcp_changed) = state
        .set_audio_settings(settings)
        .unwrap_or((false, false, false));

    if needs_outbound {
        state.restart_outbound()?;
    }
    if needs_inbound {
        state.restart_inbound()?;
    }
    if force_tcp_changed {
        if let Ok(inner) = state.inner.lock() {
            if let Some(ref handle) = inner.client_handle {
                handle.set_force_tcp(force_tcp);
            }
        }
    }

    Ok(())
}

/// Get the current voice state.
#[tauri::command]
fn get_voice_state(state: tauri::State<'_, AppState>) -> VoiceState {
    state.voice_state()
}

/// Enable voice calling for the current channel.
/// Sends unmute/undeaf to the server.
#[tauri::command]
async fn enable_voice(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.enable_voice().await
}

/// Disable voice calling (go back to deaf+muted).
#[tauri::command]
async fn disable_voice(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.disable_voice().await
}

/// Toggle mute (mic on/off, still hearing).
#[tauri::command]
async fn toggle_mute(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.toggle_mute().await
}

/// Toggle deafen (all audio on/off).
#[tauri::command]
async fn toggle_deafen(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.toggle_deafen().await
}

/// Start monitoring the microphone and emitting amplitude events.
#[tauri::command]
fn start_mic_test(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.start_mic_test()
}

/// Stop monitoring the microphone.
#[tauri::command]
fn stop_mic_test(state: tauri::State<'_, AppState>) {
    state.stop_mic_test();
}

/// Calibrate the voice activation threshold by measuring the ambient
/// noise floor for ~2 seconds (with AGC applied).  Returns the new threshold.
#[tauri::command]
async fn calibrate_voice_threshold(state: tauri::State<'_, AppState>) -> Result<f32, String> {
    state.calibrate_voice_threshold().await
}

/// Start periodic TCP pings for live latency measurement.
#[tauri::command]
fn start_latency_test(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.start_latency_test()
}

/// Stop the latency test.
#[tauri::command]
fn stop_latency_test(state: tauri::State<'_, AppState>) {
    state.stop_latency_test();
}

/// Set the user comment on the connected server (`FancyMumble` profile + bio).
#[tauri::command]
async fn set_user_comment(
    state: tauri::State<'_, AppState>,
    comment: String,
) -> Result<(), String> {
    state.set_user_comment(comment).await
}

/// Set the user avatar texture on the connected server (raw image bytes).
///
/// Accepts a JSON array of `u8` values from the frontend.
#[tauri::command]
async fn set_user_texture(
    state: tauri::State<'_, AppState>,
    texture: Vec<u8>,
) -> Result<(), String> {
    state.set_user_texture(texture).await
}

/// Return the local user's session ID assigned by the server.
#[tauri::command]
fn get_own_session(state: tauri::State<'_, AppState>) -> Option<u32> {
    state.get_own_session()
}

/// Send a plugin data transmission (e.g. polls) to the server.
///
/// `receiver_sessions` can be empty to broadcast to all users.
#[tauri::command]
async fn send_plugin_data(
    state: tauri::State<'_, AppState>,
    receiver_sessions: Vec<u32>,
    data: Vec<u8>,
    data_id: String,
) -> Result<(), String> {
    state.send_plugin_data(receiver_sessions, data, data_id).await
}

/// Update the per-channel push notification mute preferences on the server.
#[tauri::command]
async fn send_push_update(
    state: tauri::State<'_, AppState>,
    muted_channels: Vec<u32>,
) -> Result<(), String> {
    state.send_push_update(muted_channels).await
}

/// Send a live subscribe-push registration (or update) to the server.
#[tauri::command]
async fn send_subscribe_push(
    state: tauri::State<'_, AppState>,
    muted_channels: Vec<u32>,
) -> Result<(), String> {
    state.send_subscribe_push(muted_channels).await
}

/// Send a read receipt watermark to the server.
#[tauri::command]
async fn send_read_receipt(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    last_read_message_id: String,
) -> Result<(), String> {
    state
        .send_read_receipt(channel_id, last_read_message_id)
        .await
}

/// Query all read receipt states for a channel from the server.
#[tauri::command]
async fn query_read_receipts(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    state.query_read_receipts(channel_id).await
}

/// Send a WebRTC screen-sharing signaling message.
///
/// `target_session` of 0 broadcasts to all channel members.
#[tauri::command]
async fn send_webrtc_signal(
    state: tauri::State<'_, AppState>,
    target_session: u32,
    signal_type: i32,
    payload: String,
) -> Result<(), String> {
    state.send_webrtc_signal(target_session, signal_type, payload).await
}

/// Send a reaction (add/remove) on a persisted chat message.
#[tauri::command]
async fn send_reaction(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    message_id: String,
    emoji: String,
    action: String,
) -> Result<(), String> {
    state.send_reaction(channel_id, message_id, emoji, action).await
}

/// Delete persisted chat messages on the server.
///
/// At least one of `message_ids`, `time_from`/`time_to`, or `sender_hash`
/// must be provided.
#[tauri::command]
async fn delete_pchat_messages(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    message_ids: Vec<String>,
    time_from: Option<u64>,
    time_to: Option<u64>,
    sender_hash: Option<String>,
) -> Result<(), String> {
    state
        .delete_pchat_messages(channel_id, message_ids, time_from, time_to, sender_hash)
        .await
}

// --- Direct message (DM) commands --------------------------------

/// Send a direct message to a specific user.
#[tauri::command]
async fn send_dm(
    state: tauri::State<'_, AppState>,
    target_session: u32,
    body: String,
) -> Result<(), String> {
    state.send_dm(target_session, body).await
}

/// Get DM messages for a conversation with a specific user.
#[tauri::command]
fn get_dm_messages(state: tauri::State<'_, AppState>, session: u32) -> Vec<ChatMessage> {
    state.dm_messages(session)
}

/// Select a DM conversation for viewing.
#[tauri::command]
fn select_dm_user(state: tauri::State<'_, AppState>, session: u32) -> Result<(), String> {
    state.select_dm_user(session)
}

/// Get DM unread counts per user session.
#[tauri::command]
fn get_dm_unread_counts(state: tauri::State<'_, AppState>) -> HashMap<u32, u32> {
    state.dm_unread_counts()
}

/// Mark DMs with a specific user as read.
#[tauri::command]
fn mark_dm_read(state: tauri::State<'_, AppState>, session: u32) {
    state.mark_dm_read(session);
}

// --- Group chat commands --------------------------------------

/// Create a new group chat with the given name and member sessions.
#[tauri::command]
async fn create_group(
    state: tauri::State<'_, AppState>,
    name: String,
    member_sessions: Vec<u32>,
) -> Result<GroupChat, String> {
    state.create_group(name, member_sessions).await
}

/// Get all known group chats.
#[tauri::command]
fn get_groups(state: tauri::State<'_, AppState>) -> Vec<GroupChat> {
    state.groups()
}

/// Get messages for a specific group chat.
#[tauri::command]
fn get_group_messages(state: tauri::State<'_, AppState>, group_id: String) -> Vec<ChatMessage> {
    state.group_messages(&group_id)
}

/// Select a group chat for viewing.
#[tauri::command]
fn select_group(state: tauri::State<'_, AppState>, group_id: String) -> Result<(), String> {
    state.select_group(group_id)
}

/// Send a message to a group chat.
#[tauri::command]
async fn send_group_message(
    state: tauri::State<'_, AppState>,
    group_id: String,
    body: String,
) -> Result<(), String> {
    state.send_group_message(group_id, body).await
}

/// Get group unread counts.
#[tauri::command]
fn get_group_unread_counts(state: tauri::State<'_, AppState>) -> HashMap<String, u32> {
    state.group_unread_counts()
}

/// Mark a group chat as read.
#[tauri::command]
fn mark_group_read(state: tauri::State<'_, AppState>, group_id: String) {
    state.mark_group_read(&group_id);
}

/// Enable or disable native OS notifications.
#[tauri::command]
fn set_notifications_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    state.inner.lock().map_err(|e| e.to_string())?.notifications_enabled = enabled;
    Ok(())
}

/// Enable or disable dual-path sending for encrypted channels.
///
/// When disabled, the plain `TextMessage` body is replaced with a
/// placeholder so the server never sees the cleartext content.
#[tauri::command]
fn set_disable_dual_path(
    state: tauri::State<'_, AppState>,
    disabled: bool,
) -> Result<(), String> {
    state.inner.lock().map_err(|e| e.to_string())?.disable_dual_path = disabled;
    Ok(())
}

/// Change the log level filter at runtime.
///
/// Accepts a `tracing_subscriber::EnvFilter`-compatible string such as
/// `"debug"`, `"mumble_tauri=debug,mumble_protocol=debug,info"`, or
/// `"trace"`.  Returns the filter that was actually applied.
#[tauri::command]
fn set_log_level(filter: String) -> Result<String, String> {
    let handle = LOG_RELOAD_HANDLE
        .get()
        .ok_or_else(|| "logging not initialised".to_string())?;
    let new_filter =
        EnvFilter::try_new(&filter).map_err(|e| format!("invalid filter '{filter}': {e}"))?;
    let applied = format!("{new_filter}");
    handle
        .reload(new_filter)
        .map_err(|e| format!("failed to reload filter: {e}"))?;
    tracing::info!(filter = %applied, "log level changed");
    Ok(applied)
}

/// Reset all app data to factory defaults (preferences, saved servers, certs).
#[tauri::command]
async fn reset_app_data(app: tauri::AppHandle) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    // Remove known data files.
    for name in &["preferences.json", "servers.json", "passwords.json"] {
        let path = data_dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    // Remove certs directory.
    let certs = data_dir.join("certs");
    if certs.exists() {
        std::fs::remove_dir_all(&certs).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Set the taskbar badge count.
///
/// On Windows this renders a small red overlay icon with the count (the native
/// `set_badge_count` API is not supported). On Linux/macOS it delegates to
/// the native badge-count API. On Android/iOS this is a no-op.
#[tauri::command]
fn update_badge_count(window: tauri::Window, count: Option<u32>) -> Result<(), String> {
    set_badge_platform(&window, count)
}

/// Windows implementation - overlay icon.
#[cfg(target_os = "windows")]
fn set_badge_platform(window: &tauri::Window, count: Option<u32>) -> Result<(), String> {
    match count.filter(|&c| c > 0) {
        Some(c) => {
            let rgba = render_badge_icon(c);
            let image = tauri::image::Image::new_owned(rgba, 16, 16);
            window.set_overlay_icon(Some(image)).map_err(|e| e.to_string())
        }
        None => window.set_overlay_icon(None).map_err(|e| e.to_string()),
    }
}

/// Linux/macOS implementation - native badge count.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn set_badge_platform(window: &tauri::Window, count: Option<u32>) -> Result<(), String> {
    let badge = count.filter(|&c| c > 0).map(i64::from);
    window.set_badge_count(badge).map_err(|e| e.to_string())
}

/// Android/iOS - badge counts are not supported, no-op.
#[cfg(any(target_os = "android", target_os = "ios"))]
fn set_badge_platform(_window: &tauri::Window, _count: Option<u32>) -> Result<(), String> {
    Ok(())
}

// --- Content offloading commands ----------------------------------

/// Encrypt a heavy message body and write it to a temp file, replacing
/// the in-memory body with a lightweight placeholder.
///
/// `scope` is `"channel"`, `"dm"`, or `"group"`.
/// `scope_id` is the channel ID, DM session, or group UUID as a string.
#[tauri::command]
fn offload_message(
    state: tauri::State<'_, AppState>,
    message_id: String,
    scope: String,
    scope_id: String,
) -> Result<(), String> {
    state.offload_message(message_id, scope, scope_id)
}

/// Decrypt an offloaded message body from its temp file and restore it
/// in the in-memory message store.  Returns the restored body.
#[tauri::command]
fn load_offloaded_message(
    state: tauri::State<'_, AppState>,
    message_id: String,
    scope: String,
    scope_id: String,
) -> Result<String, String> {
    state.load_offloaded_message(message_id, scope, scope_id)
}

/// Decrypt multiple offloaded message bodies in a single IPC call.
///
/// Returns a map of `message_id` to restored body.  Keys that fail to
/// decrypt are silently omitted from the result.
#[tauri::command]
fn load_offloaded_messages_batch(
    state: tauri::State<'_, AppState>,
    message_ids: Vec<String>,
    scope: String,
    scope_id: String,
) -> Result<HashMap<String, String>, String> {
    state.load_offloaded_messages_batch(message_ids, scope, scope_id)
}

/// Delete all offloaded temp files.
#[tauri::command]
fn clear_offloaded_messages(state: tauri::State<'_, AppState>) {
    state.clear_offloaded();
}

/// Send a `PchatFetch` request for older messages (pagination).
/// The response arrives asynchronously via the `PchatFetchResponse` handler
/// which emits `"pchat-fetch-complete"` and `"new-message"` events.
#[tauri::command]
async fn fetch_older_messages(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    before_id: Option<String>,
    limit: u32,
) -> Result<(), String> {
    state.fetch_older_messages(channel_id, before_id, limit).await
}

/// Collect debug statistics for the developer info panel.
#[tauri::command]
fn get_debug_stats(state: tauri::State<'_, AppState>) -> DebugStats {
    state.debug_stats()
}

// -- Admin commands ----------------------------------------------

/// Kick a user from the server.
#[tauri::command]
async fn kick_user(
    state: tauri::State<'_, AppState>,
    session: u32,
    reason: Option<String>,
) -> Result<(), String> {
    state.kick_user(session, reason).await
}

/// Ban a user from the server.
#[tauri::command]
async fn ban_user(
    state: tauri::State<'_, AppState>,
    session: u32,
    reason: Option<String>,
) -> Result<(), String> {
    state.ban_user(session, reason).await
}

/// Register a user on the server using their current certificate.
#[tauri::command]
async fn register_user(
    state: tauri::State<'_, AppState>,
    session: u32,
) -> Result<(), String> {
    state.register_user(session).await
}

/// Admin-mute or unmute another user.
#[tauri::command]
async fn mute_user(
    state: tauri::State<'_, AppState>,
    session: u32,
    muted: bool,
) -> Result<(), String> {
    state.mute_user(session, muted).await
}

/// Admin-deafen or undeafen another user.
#[tauri::command]
async fn deafen_user(
    state: tauri::State<'_, AppState>,
    session: u32,
    deafened: bool,
) -> Result<(), String> {
    state.deafen_user(session, deafened).await
}

/// Set or clear priority speaker for another user.
#[tauri::command]
async fn set_priority_speaker(
    state: tauri::State<'_, AppState>,
    session: u32,
    priority: bool,
) -> Result<(), String> {
    state.set_priority_speaker(session, priority).await
}

/// Reset another user's comment (admin action).
#[tauri::command]
async fn reset_user_comment(
    state: tauri::State<'_, AppState>,
    session: u32,
) -> Result<(), String> {
    state.reset_user_comment(session).await
}

/// Remove another user's avatar (admin action).
#[tauri::command]
async fn remove_user_avatar(
    state: tauri::State<'_, AppState>,
    session: u32,
) -> Result<(), String> {
    state.remove_user_avatar(session).await
}

/// Request ping/connection statistics for a specific user.
///
/// The server responds asynchronously with a `UserStats` message,
/// which is emitted to the frontend as a `"user-stats"` event.
#[tauri::command]
async fn request_user_stats(
    state: tauri::State<'_, AppState>,
    session: u32,
) -> Result<(), String> {
    state.request_user_stats(session).await
}

/// Request the registered user list from the server.
#[tauri::command]
async fn request_user_list(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.request_user_list().await
}

/// Send updated user-list entries (rename / delete) to the server.
#[tauri::command]
async fn update_user_list(
    state: tauri::State<'_, AppState>,
    users: Vec<state::types::RegisteredUserUpdate>,
) -> Result<(), String> {
    state.update_user_list(users).await
}

/// Request the ban list from the server.
#[tauri::command]
async fn request_ban_list(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.request_ban_list().await
}

/// Send an updated ban list to the server (replaces the full list).
#[tauri::command]
async fn update_ban_list(
    state: tauri::State<'_, AppState>,
    bans: Vec<state::types::BanEntryInput>,
) -> Result<(), String> {
    state.update_ban_list(bans).await
}

/// Request the ACL for a specific channel.
#[tauri::command]
async fn request_acl(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    state.request_acl(channel_id).await
}

/// Send an updated ACL for a channel to the server.
#[tauri::command]
async fn update_acl(
    state: tauri::State<'_, AppState>,
    acl: state::types::AclInput,
) -> Result<(), String> {
    state.update_acl(acl).await
}

/// Confirm the initial custodian list for a channel (TOFU, Section 5.7).
#[tauri::command]
fn confirm_custodians(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    let mut shared = state.inner.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut pchat) = shared.pchat {
        pchat.key_manager.confirm_custodian_list(channel_id);
    }
    Ok(())
}

/// Accept a pending custodian list change for a channel (Section 5.7).
#[tauri::command]
fn accept_custodian_changes(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    let mut shared = state.inner.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut pchat) = shared.pchat {
        pchat.key_manager.accept_custodian_update(channel_id);
    }
    Ok(())
}

/// Approve a pending key-share request: actually send the encrypted
/// channel key to the peer that triggered the consent banner.
#[tauri::command]
async fn approve_key_share(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    peer_cert_hash: String,
) -> Result<(), String> {
    use mumble_protocol::persistent::PchatProtocol;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Extract everything we need while holding the lock, then release it.
    let (handle, exchange, share_requests_emit) = {
        let mut shared = state.inner.lock().map_err(|e| e.to_string())?;

        // Remove the pending entry and capture its request_id.
        let idx = shared
            .pending_key_shares
            .iter()
            .position(|p| p.channel_id == channel_id && p.peer_cert_hash == peer_cert_hash)
            .ok_or("no pending key share for this channel/peer")?;
        let removed = shared.pending_key_shares.remove(idx);
        let request_id = removed.request_id;

        // Collect payload for deferred emit outside the lock.
        let share_requests_emit = shared.tauri_app_handle.as_ref().map(|app| {
            let remaining: Vec<_> = shared
                .pending_key_shares
                .iter()
                .filter(|p| p.channel_id == channel_id)
                .cloned()
                .collect();
            (
                app.clone(),
                state::types::KeyShareRequestsChangedPayload {
                    channel_id,
                    pending: remaining,
                },
            )
        });

        let pchat = shared.pchat.as_ref().ok_or("pchat not initialised")?;

        let peer_record = pchat
            .key_manager
            .get_peer(&peer_cert_hash)
            .ok_or("peer public key not known")?;
        let peer_x25519 = peer_record.dh_public;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let mut wire_exchange = pchat
            .key_manager
            .distribute_key(
                channel_id,
                PchatProtocol::FancyV1FullArchive,
                0,
                &peer_cert_hash,
                &peer_x25519,
                request_id.as_deref(),
                now_ms,
            )
            .map_err(|e| format!("failed to build key exchange: {e}"))?;

        wire_exchange.sender_hash = pchat.own_cert_hash.clone();

        let proto =
            state::pchat::wire_key_exchange_to_proto(&wire_exchange);

        let handle = shared
            .client_handle
            .clone()
            .ok_or("not connected")?;

        (handle, proto, share_requests_emit)
    };

    // Emit outside the lock to avoid deadlock with Tauri IPC.
    if let Some((app, payload)) = share_requests_emit {
        use tauri::Emitter;
        let _ = app.emit("pchat-key-share-requests-changed", payload);
    }

    // Send the key exchange to the peer.
    handle
        .send(mumble_protocol::command::SendPchatKeyExchange { exchange })
        .await
        .map_err(|e| format!("send failed: {e}"))?;

    // Record the peer as a key holder locally so we don't prompt consent
    // for them again on subsequent channel moves.
    if let Ok(mut shared) = state.inner.lock() {
        if let Some(ref mut pchat) = shared.pchat {
            pchat
                .key_manager
                .record_key_holder(channel_id, peer_cert_hash.clone());
        }
    }

    // Report to the server that the peer now holds the key.
    let report = mumble_protocol::proto::mumble_tcp::PchatKeyHolderReport {
        channel_id: Some(channel_id),
        cert_hash: Some(peer_cert_hash),
        takeover_mode: None,
    };
    let _ = handle
        .send(mumble_protocol::command::SendPchatKeyHolderReport { report })
        .await;

    Ok(())
}

/// Dismiss a pending key-share request without sending the key.
#[tauri::command]
fn dismiss_key_share(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    peer_cert_hash: String,
) -> Result<(), String> {
    let share_requests_emit = {
        let mut shared = state.inner.lock().map_err(|e| e.to_string())?;

        shared
            .pending_key_shares
            .retain(|p| !(p.channel_id == channel_id && p.peer_cert_hash == peer_cert_hash));

        // Collect payload for deferred emit outside the lock.
        shared.tauri_app_handle.as_ref().map(|app| {
            let remaining: Vec<_> = shared
                .pending_key_shares
                .iter()
                .filter(|p| p.channel_id == channel_id)
                .cloned()
                .collect();
            (
                app.clone(),
                state::types::KeyShareRequestsChangedPayload {
                    channel_id,
                    pending: remaining,
                },
            )
        })
    };

    // Emit outside the lock to avoid deadlock with Tauri IPC.
    if let Some((app, payload)) = share_requests_emit {
        use tauri::Emitter;
        let _ = app.emit("pchat-key-share-requests-changed", payload);
    }

    Ok(())
}

/// Ask the server for the list of key holders for a channel.
#[tauri::command]
async fn query_key_holders(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    let handle = {
        let shared = state.inner.lock().map_err(|e| e.to_string())?;
        shared.client_handle.clone().ok_or("not connected")?
    };
    let query = mumble_protocol::proto::mumble_tcp::PchatKeyHoldersQuery {
        channel_id: Some(channel_id),
    };
    handle
        .send(mumble_protocol::command::SendPchatKeyHoldersQuery { query })
        .await
        .map_err(|e| format!("send failed: {e}"))
}

/// Return the cached key holders for a channel (from the last server response).
#[tauri::command]
fn get_key_holders(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Vec<state::types::KeyHolderEntry> {
    let shared = state.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    shared.key_holders.get(&channel_id).cloned().unwrap_or_default()
}

/// Request a key-ownership takeover for a channel (requires `KeyOwner` permission).
///
/// `mode` must be `"full_wipe"` (delete messages + key takeover) or
/// `"key_only"` (key takeover without deleting messages).
///
/// On success the server responds with an updated `PchatKeyHoldersList`.
/// On failure the server sends `PermissionDenied`.
#[tauri::command]
async fn key_takeover(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    mode: String,
) -> Result<(), String> {
    use mumble_protocol::proto::mumble_tcp::pchat_key_holder_report::KeyTakeoverMode;
    let takeover_mode = match mode.as_str() {
        "full_wipe" => KeyTakeoverMode::FullWipe,
        "key_only" => KeyTakeoverMode::KeyOnly,
        _ => return Err(format!("invalid takeover mode: {mode}")),
    };
    state::pchat::send_key_takeover(&state.inner, channel_id, takeover_mode);
    Ok(())
}

// --- Image processing --------------------------------------------

// --- Image processing commands -----------------------------------

/// Apply a Gaussian blur to an image.
///
/// `image_base64` is the raw file content encoded as a base64 string.
/// `sigma` controls the blur strength (typical range 1.0 - 30.0).
/// Returns base64-encoded JPEG bytes.
///
/// Runs on a dedicated blocking thread so the async runtime (and Tauri IPC)
/// stays responsive while the CPU-heavy image processing executes.
#[tauri::command]
async fn blur_image(image_base64: String, sigma: f32) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use fancy_utils::image_filter::{BlurFilter, ImageFilter};

        let image_bytes = STANDARD
            .decode(&image_base64)
            .map_err(|e| format!("Failed to decode base64 input: {e}"))?;

        let result = BlurFilter::new(sigma)
            .apply(&image_bytes)
            .map_err(|e| e.to_string())?;
        Ok(STANDARD.encode(result))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Process a chat background image by applying blur and/or dim in one pass.
///
/// `image_base64` is the raw file content encoded as a base64 string.
/// `sigma` controls blur strength (0 = no blur, typical range 1.0 - 30.0).
/// `dim` controls darkening (0.0 = no dim, 1.0 = fully black).
/// Returns base64-encoded JPEG bytes.
///
/// The image is downscaled to 960x540 before processing to keep blur fast.
/// Since the result is used as a blurred/dimmed background, the reduced
/// resolution is imperceptible.
///
/// Runs on a dedicated blocking thread so the async runtime (and Tauri IPC)
/// stays responsive while the CPU-heavy image processing executes.
#[tauri::command]
async fn process_background(
    image_base64: String,
    sigma: f32,
    dim: f32,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use fancy_utils::image_filter::{process_pipeline, BlurFilter, DimFilter, ImageTransform};

        let image_bytes = STANDARD
            .decode(&image_base64)
            .map_err(|e| format!("Failed to decode base64 input: {e}"))?;

        let blur = BlurFilter::new(sigma);
        let dim_filter = DimFilter::new(dim);

        let mut transforms: Vec<&dyn ImageTransform> = Vec::new();
        if sigma > 0.0 {
            transforms.push(&blur);
        }
        if dim > 0.0 {
            transforms.push(&dim_filter);
        }

        if transforms.is_empty() {
            // No processing needed, but re-encode to JPEG for consistency.
            let result = process_pipeline(&image_bytes, &[], false)
                .map_err(|e| e.to_string())?;
            return Ok(STANDARD.encode(result));
        }

        let result =
            process_pipeline(&image_bytes, &transforms, true).map_err(|e| e.to_string())?;
        Ok(STANDARD.encode(result))
    })
    .await
    .map_err(|e| e.to_string())?
}

// --- Application bootstrap ---------------------------------------

/// Probe whether a usable EGL display is available on this system.
///
/// Loads `libEGL.so.1` at runtime and calls `eglGetDisplay(EGL_DEFAULT_DISPLAY)`.
/// Returns `true` only when the call succeeds and returns a non-null display.
/// This avoids the hard `abort()` that `WebKitGTK` triggers when hardware
/// acceleration is forced on a system without working EGL (VMs, containers,
/// broken GPU drivers).
#[cfg(target_os = "linux")]
fn egl_display_available() -> bool {
    const EGL_DEFAULT_DISPLAY: *mut std::ffi::c_void = std::ptr::null_mut();
    const EGL_NO_DISPLAY: *mut std::ffi::c_void = std::ptr::null_mut();

    #[allow(unsafe_code, reason = "probing EGL via dlopen/dlsym is inherently unsafe FFI")]
    unsafe {
        let lib = libc::dlopen(
            c"libEGL.so.1".as_ptr(),
            libc::RTLD_NOW | libc::RTLD_NOLOAD,
        );
        let lib = if lib.is_null() {
            libc::dlopen(c"libEGL.so.1".as_ptr(), libc::RTLD_NOW)
        } else {
            lib
        };
        if lib.is_null() {
            return false;
        }
        let sym = libc::dlsym(lib, c"eglGetDisplay".as_ptr());
        if sym.is_null() {
            let _ = libc::dlclose(lib);
            return false;
        }
        let get_display: extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void =
            std::mem::transmute(sym);
        let display = get_display(EGL_DEFAULT_DISPLAY);
        let _ = libc::dlclose(lib);
        display != EGL_NO_DISPLAY
    }
}

/// Configure `WebKitGTK` hardware acceleration and `WebGL`.
///
/// Forces `HardwareAccelerationPolicy::Always` when a working EGL display
/// is detected so that CSS `backdrop-filter: blur()` renders correctly.
/// Falls back to the default `OnDemand` policy otherwise.
#[cfg(target_os = "linux")]
fn configure_webkitgtk(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else {
        tracing::warn!("WebKitGTK: main webview window not found in setup");
        return;
    };
    let has_egl = egl_display_available();
    let result = window.with_webview(move |webview| {
        use webkit2gtk::{SettingsExt, WebViewExt};
        let wv = webview.inner();
        let Some(settings) = wv.settings() else {
            tracing::warn!("WebKitGTK: could not get webview settings");
            return;
        };
        if has_egl {
            settings.set_hardware_acceleration_policy(
                webkit2gtk::HardwareAccelerationPolicy::Always,
            );
            settings.set_enable_webgl(true);
            tracing::info!("WebKitGTK: hardware acceleration set to Always, WebGL enabled");
        } else {
            tracing::warn!(
                "WebKitGTK: EGL display not available, keeping default acceleration policy"
            );
        }
    });
    if let Err(e) = result {
        tracing::warn!("WebKitGTK: with_webview failed: {e}");
    }
}

/// Entry point for the Tauri application.
///
/// Initialises the TLS crypto provider, sets up logging, registers all
/// Tauri commands, and starts the application event loop.
#[allow(clippy::too_many_lines, reason = "application bootstrap registers all Tauri commands, plugins, and event handlers")]
#[allow(clippy::expect_used, reason = "Tauri builder failure during startup is unrecoverable")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Install the ring TLS crypto provider before anything touches rustls.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // On Linux, handle quick-action CLI args (e.g. `--action mute`) sent
    // by .desktop file actions.  If one is found, forward it to the running
    // instance via a Unix socket and exit immediately.
    #[cfg(target_os = "linux")]
    if linux_desktop::try_send_quick_action() {
        return;
    }

    // On Linux, set the GTK program name and application name so GNOME
    // matches the running window to the .desktop file.
    #[cfg(target_os = "linux")]
    linux_desktop::set_gtk_identifiers();

    // On Linux, force WebKitGTK to use compositing mode so that CSS
    // `backdrop-filter: blur()` is actually rendered (not just parsed).
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("WEBKIT_FORCE_COMPOSITING_MODE", "1");
    }

    // Set up a reloadable tracing subscriber so the log level can be changed
    // at runtime from the frontend (Advanced Settings > Debug Logging).
    let default_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    let filter = EnvFilter::try_new(&default_filter).unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, reload_handle) = reload::Layer::new(filter);
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer())
        .init();
    let _ = LOG_RELOAD_HANDLE.set(reload_handle);

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init());

    // Register the foreground connection service plugin on Android so
    // the process stays alive (and keeps receiving messages / showing
    // notifications) when the app is in the background.
    #[cfg(target_os = "android")]
    let builder = builder.plugin(
        tauri::plugin::Builder::<tauri::Wry, ()>::new("connection-service")
            .setup(|app, api| {
                let handle = api.register_android_plugin(
                    "com.fancymumble.app",
                    "ConnectionServicePlugin",
                )?;
                let cs_handle = connection_service::ConnectionServiceHandle(handle);
                connection_service::register_disconnect_listener(&cs_handle, app.clone());
                connection_service::register_navigate_listener(&cs_handle, app.clone());
                let _ = app.manage(cs_handle);
                Ok(())
            })
            .build(),
    );

    // Register the FCM plugin on Android so the Rust backend can
    // subscribe/unsubscribe to FCM topics for push notifications.
    #[cfg(target_os = "android")]
    let builder = builder.plugin(
        tauri::plugin::Builder::<tauri::Wry, ()>::new("fcm-service")
            .setup(|app, api| {
                let handle = api.register_android_plugin(
                    "com.fancymumble.app",
                    "FcmPlugin",
                )?;
                let fcm_handle = fcm_service::FcmPluginHandle(handle);
                let _ = app.manage(fcm_handle);
                Ok(())
            })
            .build(),
    );

    // Window state persistence is desktop-only.
    #[cfg(not(target_os = "android"))]
    let builder = builder.plugin(tauri_plugin_window_state::Builder::new().build());

    // Global shortcuts (PTT) are only available on desktop.
    #[cfg(not(target_os = "android"))]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());

    builder
        .manage(AppState::new())
        .setup(|app| {
            let state = app.state::<AppState>();
            state.set_app_handle(app.handle().clone());
            // Initialise the encrypted temp-file store for message offloading.
            // Stale files from a previous session are deleted first.
            if let Err(e) = state.init_offload_store() {
                tracing::warn!("Failed to initialise offload store: {e}");
            }

            // On Linux, install the .desktop file (for GNOME app name + icon)
            // and start the quick-action IPC listener.
            #[cfg(target_os = "linux")]
            {
                linux_desktop::install_desktop_entry();
                linux_desktop::start_action_listener(app.handle().clone());
            }

            #[cfg(target_os = "linux")]
            configure_webkitgtk(app);

            // System tray icon with quick actions (desktop only).
            #[cfg(not(target_os = "android"))]
            if let Err(e) = tray::setup_tray(app) {
                tracing::warn!("Failed to create system tray icon: {e}");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connect,
            generate_certificate,
            list_certificates,
            delete_certificate,
            export_certificate,
            import_certificate,
            disconnect,
            get_status,
            get_channels,
            get_users,
            get_messages,
            send_message,
            select_channel,
            join_channel,
            get_current_channel,
            toggle_listen,
            get_listened_channels,
            get_push_subscribed_channels,
            get_unread_counts,
            mark_channel_read,
            get_server_config,
            get_server_info,
            get_welcome_text,
            update_channel,
            create_channel,
            delete_channel,
            ping_server,
            fetch_public_servers,
            get_audio_devices,
            get_output_devices,
            get_audio_settings,
            set_audio_settings,
            get_voice_state,
            enable_voice,
            disable_voice,
            toggle_mute,
            toggle_deafen,
            start_mic_test,
            stop_mic_test,
            calibrate_voice_threshold,
            start_latency_test,
            stop_latency_test,
            set_user_comment,
            set_user_texture,
            get_own_session,
            send_plugin_data,
            send_push_update,
            send_subscribe_push,
            send_read_receipt,
            query_read_receipts,
            send_webrtc_signal,
            send_reaction,
            delete_pchat_messages,
            send_dm,
            get_dm_messages,
            select_dm_user,
            get_dm_unread_counts,
            mark_dm_read,
            create_group,
            get_groups,
            get_group_messages,
            select_group,
            send_group_message,
            get_group_unread_counts,
            mark_group_read,
            reset_app_data,
            set_log_level,
            set_notifications_enabled,
            set_disable_dual_path,
            update_badge_count,
            get_system_clock_format,
            offload_message,
            load_offloaded_message,
            load_offloaded_messages_batch,
            clear_offloaded_messages,
            fetch_older_messages,
            get_debug_stats,
            super_search,
            get_photos,
            kick_user,
            ban_user,
            register_user,
            mute_user,
            deafen_user,
            set_priority_speaker,
            reset_user_comment,
            remove_user_avatar,
            request_user_stats,
            request_user_list,
            update_user_list,
            request_ban_list,
            update_ban_list,
            request_acl,
            update_acl,
            confirm_custodians,
            accept_custodian_changes,
            approve_key_share,
            dismiss_key_share,
            query_key_holders,
            get_key_holders,
            key_takeover,
            blur_image,
            process_background,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(focused) = event {
                if let Some(state) = window.try_state::<AppState>() {
                    if let Ok(mut s) = state.inner.lock() {
                        s.app_focused = *focused;
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Clean up offloaded temp files on graceful shutdown.
                if let Some(state) = app.try_state::<AppState>() {
                    state.shutdown_offload_store();
                }
                // Remove the quick-action Unix socket.
                #[cfg(target_os = "linux")]
                linux_desktop::cleanup_socket();
            }
        });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn parse_single_server() {
        let xml = r#"<servers><server name="Test Server" ca="1" continent_code="EU" country="Germany" country_code="DE" ip="mumble.example.com" port="64738" region="Bavaria" url="https://example.com"/></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(
            servers[0],
            PublicServer {
                name: "Test Server".into(),
                country: "Germany".into(),
                country_code: "DE".into(),
                ip: "mumble.example.com".into(),
                port: 64738,
                region: "Bavaria".into(),
                url: "https://example.com".into(),
            }
        );
    }

    #[test]
    fn parse_multiple_servers() {
        let xml = r#"<servers>
            <server name="Alpha" ca="0" continent_code="NA" country="Canada" country_code="CA" ip="1.2.3.4" port="12345" region="Ontario" url="https://alpha.ca"/>
            <server name="Beta" ca="1" continent_code="AS" country="Japan" country_code="JP" ip="5.6.7.8" port="64738" region="Tokyo" url="https://beta.jp"/>
            <server name="Gamma" ca="0" continent_code="EU" country="France" country_code="FR" ip="fr.example.com" port="9999" region="Paris" url=""/>
        </servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers.len(), 3);
        assert_eq!(servers[0].name, "Alpha");
        assert_eq!(servers[0].country_code, "CA");
        assert_eq!(servers[0].port, 12345);
        assert_eq!(servers[1].name, "Beta");
        assert_eq!(servers[1].country, "Japan");
        assert_eq!(servers[2].name, "Gamma");
        assert_eq!(servers[2].ip, "fr.example.com");
        assert_eq!(servers[2].port, 9999);
    }

    #[test]
    fn parse_empty_server_list() {
        let xml = r#"<servers></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert!(servers.is_empty());
    }

    #[test]
    fn parse_self_closing_servers_tag() {
        let xml = r#"<servers/>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert!(servers.is_empty());
    }

    #[test]
    fn parse_default_port_when_missing() {
        let xml = r#"<servers><server name="NoPort" country="US" country_code="US" ip="10.0.0.1" region="Test" url=""/></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].port, 64738);
    }

    #[test]
    fn parse_special_characters_in_name() {
        let xml = r#"<servers><server name="&lt;Cool&amp;Server&gt;" country="US" country_code="US" ip="10.0.0.1" port="64738" region="Test" url=""/></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers[0].name, "<Cool&Server>");
    }

    #[test]
    fn parse_unicode_in_name() {
        let xml = r#"<servers><server name="Mumble Deutsch" country="Germany" country_code="DE" ip="10.0.0.1" port="64738" region="NRW" url=""/></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers[0].name, "Mumble Deutsch");
        assert_eq!(servers[0].country, "Germany");
    }

    #[test]
    fn parse_invalid_xml_returns_error() {
        let xml = r#"<servers><server name="broken"</servers>"#;
        let result = parse_public_server_xml(xml);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("XML parse error"));
    }

    #[test]
    fn parse_extra_attributes_are_ignored() {
        let xml = r#"<servers><server name="Extra" ca="1" continent_code="EU" country="UK" country_code="GB" ip="10.0.0.1" port="64738" region="London" url="https://uk.example.com" extra_field="ignored"/></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "Extra");
    }

    #[test]
    fn parse_realistic_snippet() {
        let xml = r#"<servers>
<server name="`JOIN RADIO BRIKER NUSANTARA`" ca="0" continent_code="AS" country="Singapore" country_code="SG" ip="beve-studio.my.id" port="10622" region="Singapore" url="https://www.mumble.info/"/>
<server name="Comms" ca="1" continent_code="EU" country="Germany" country_code="DE" ip="mumble.natenom.dev" port="64738" region="Baden-Wurttemberg" url="https://natenom.dev"/>
</servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "`JOIN RADIO BRIKER NUSANTARA`");
        assert_eq!(servers[0].country_code, "SG");
        assert_eq!(servers[0].port, 10622);
        assert_eq!(servers[1].ip, "mumble.natenom.dev");
        assert_eq!(servers[1].country, "Germany");
    }
}
