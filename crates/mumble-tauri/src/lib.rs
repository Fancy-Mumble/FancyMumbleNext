//! Tauri application entry point with Mumble backend commands.
//!
// All public command functions receive `tauri::State` by value, which is
// required by the `#[tauri::command]` macro - suppress the lint crate-wide.
#![allow(clippy::needless_pass_by_value)]

#[cfg(not(target_os = "android"))]
mod audio;
mod state;

use state::{
    AppState, AudioDevice, AudioSettings, ChannelEntry, ChatMessage, ConnectionStatus,
    DebugStats, GroupChat, SearchResult, ServerConfig, ServerInfo, UserEntry, VoiceState,
};
use std::collections::HashMap;
use tauri::Manager;

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
#[allow(unsafe_code)]
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
                    let px = ox + col;
                    let py = start_y + row;
                    if px < size && py < size {
                        let i = (py * size + px) * 4;
                        rgba[i] = 255;     // R
                        rgba[i + 1] = 255; // G
                        rgba[i + 2] = 255; // B
                        rgba[i + 3] = 255; // A
                    }
                }
            }
        }
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
) -> Result<(), String> {
    state.connect(host, port, username, cert_label).await
}

/// Generate a self-signed TLS client certificate and save it under
/// `{app_data_dir}/certs/{label}.cert.pem` and `.key.pem`.
/// Does nothing if the certificate already exists.
#[tauri::command]
async fn generate_certificate(
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    use rcgen::generate_simple_self_signed;

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let cert_dir = data_dir.join("certs");
    std::fs::create_dir_all(&cert_dir).map_err(|e| e.to_string())?;

    let cert_path = cert_dir.join(format!("{label}.cert.pem"));
    if cert_path.exists() {
        return Ok(()); // already exists
    }

    let certified = generate_simple_self_signed(vec![label.clone()])
        .map_err(|e| e.to_string())?;
    let cert_pem = certified.cert.pem();
    let key_pem = certified.signing_key.serialize_pem();

    std::fs::write(&cert_path, cert_pem).map_err(|e| e.to_string())?;
    std::fs::write(
        cert_dir.join(format!("{label}.key.pem")),
        key_pem,
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// List the labels of all certificates stored in `{app_data_dir}/certs/`.
#[tauri::command]
async fn list_certificates(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let cert_dir = data_dir.join("certs");
    if !cert_dir.exists() {
        return Ok(vec![]);
    }

    let mut labels = Vec::new();
    for entry in std::fs::read_dir(&cert_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(label) = name.strip_suffix(".cert.pem") {
            labels.push(label.to_string());
        }
    }
    labels.sort();
    Ok(labels)
}

/// Delete a certificate pair by label.
#[tauri::command]
async fn delete_certificate(
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let cert_dir = data_dir.join("certs");
    let cert_path = cert_dir.join(format!("{label}.cert.pem"));
    let key_path = cert_dir.join(format!("{label}.key.pem"));
    if cert_path.exists() {
        std::fs::remove_file(&cert_path).map_err(|e| e.to_string())?;
    }
    if key_path.exists() {
        std::fs::remove_file(&key_path).map_err(|e| e.to_string())?;
    }
    Ok(())
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
fn super_search(state: tauri::State<'_, AppState>, query: String) -> Vec<SearchResult> {
    state.super_search(&query)
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
fn select_channel(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    state.select_channel(channel_id)
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

/// Update a channel's name and/or description on the server.
#[tauri::command]
async fn update_channel(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    name: Option<String>,
    description: Option<String>,
) -> Result<(), String> {
    state.update_channel(channel_id, name, description).await
}

/// Ping a Mumble server by attempting a TCP connection and measuring
/// how long it takes. Times out after 4 seconds.
#[tauri::command]
async fn ping_server(host: String, port: u16) -> PingResult {
    use std::time::Instant;
    use tokio::net::TcpStream;
    use tokio::time::{timeout, Duration};

    let addr = format!("{host}:{port}");
    let start = Instant::now();

    match timeout(Duration::from_secs(4), TcpStream::connect(&addr)).await {
        Ok(Ok(_stream)) => PingResult {
            online: true,
            latency_ms: Some(start.elapsed().as_millis() as u32),
        },
        _ => PingResult {
            online: false,
            latency_ms: None,
        },
    }
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

    tracing::info!("Fetched {} public servers", servers.len());
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
    let (needs_outbound, needs_inbound) = state
        .set_audio_settings(settings)
        .unwrap_or((false, false));

    if needs_outbound {
        state.restart_outbound()?;
    }
    if needs_inbound {
        state.restart_inbound()?;
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

/// Reset all app data to factory defaults (preferences, saved servers, certs).
#[tauri::command]
async fn reset_app_data(app: tauri::AppHandle) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    // Remove known data files.
    for name in &["preferences.json", "servers.json"] {
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

/// Collect debug statistics for the developer info panel.
#[tauri::command]
fn get_debug_stats(state: tauri::State<'_, AppState>) -> DebugStats {
    state.debug_stats()
}

// ── Admin commands ──────────────────────────────────────────────

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

// --- Application bootstrap ---------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Install the ring TLS crypto provider before anything touches rustls.
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt::init();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_opener::init());

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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connect,
            generate_certificate,
            list_certificates,
            delete_certificate,
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
            get_unread_counts,
            mark_channel_read,
            get_server_config,
            get_server_info,
            get_welcome_text,
            update_channel,
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
            start_latency_test,
            stop_latency_test,
            set_user_comment,
            set_user_texture,
            get_own_session,
            send_plugin_data,
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
            update_badge_count,
            get_system_clock_format,
            offload_message,
            load_offloaded_message,
            load_offloaded_messages_batch,
            clear_offloaded_messages,
            get_debug_stats,
            super_search,
            kick_user,
            ban_user,
            mute_user,
            deafen_user,
            set_priority_speaker,
            reset_user_comment,
            remove_user_avatar,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Clean up offloaded temp files on graceful shutdown.
                if let Some(state) = app.try_state::<AppState>() {
                    state.shutdown_offload_store();
                }
            }
        });
}

#[cfg(test)]
mod tests {
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
