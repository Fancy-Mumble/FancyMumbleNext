//! Tauri application entry point with Mumble backend commands.
//!
// All public command functions receive `tauri::State` by value, which is
// required by the `#[tauri::command]` macro - suppress the lint crate-wide.
#![allow(clippy::needless_pass_by_value)]

mod audio;
mod state;

use state::{
    AppState, AudioDevice, AudioSettings, ChannelEntry, ChatMessage, ConnectionStatus,
    ServerConfig, UserEntry, VoiceState,
};
use std::collections::HashMap;
use tauri::Manager;

/// Result of a server ping attempt.
#[derive(serde::Serialize, Clone)]
struct PingResult {
    online: bool,
    /// Round-trip time in milliseconds (None when offline).
    latency_ms: Option<u32>,
}

// ─── Tauri commands ───────────────────────────────────────────────

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
    let key_pem = certified.key_pair.serialize_pem();

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

// ─── Audio device commands ────────────────────────────────────────

/// List available audio input devices (microphones).
#[tauri::command]
fn get_audio_devices() -> Vec<AudioDevice> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok());

    host.input_devices()
        .map(|devices| {
            devices
                .filter_map(|d| {
                    let name = d.name().ok()?;
                    Some(AudioDevice {
                        name: name.clone(),
                        is_default: default_name.as_deref() == Some(&name),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Get current audio settings.
#[tauri::command]
fn get_audio_settings(state: tauri::State<'_, AppState>) -> AudioSettings {
    state.audio_settings()
}

/// Update audio settings.
#[tauri::command]
fn set_audio_settings(
    state: tauri::State<'_, AppState>,
    settings: AudioSettings,
) {
    state.set_audio_settings(settings);
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

// ─── Application bootstrap ───────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Install the ring TLS crypto provider before anything touches rustls.
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::new())
        .setup(|app| {
            let state = app.state::<AppState>();
            state.set_app_handle(app.handle().clone());
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
            ping_server,
            get_audio_devices,
            get_audio_settings,
            set_audio_settings,
            get_voice_state,
            enable_voice,
            disable_voice,
            toggle_mute,
            toggle_deafen,
            set_user_comment,
            set_user_texture,
            get_own_session,
            send_plugin_data,
            reset_app_data,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
