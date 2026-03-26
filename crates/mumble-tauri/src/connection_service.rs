//! Bridge to the Android `ConnectionService` foreground service.
//!
//! On Android, when the app goes to the background, the OS may suspend
//! or kill the process.  This drops the TCP connection to the Mumble
//! server, so no messages are received and no notifications are shown.
//!
//! A foreground service with a persistent notification keeps the
//! process alive and exempt from Doze-mode network restrictions.

use crate::state::AppState;
use tauri::plugin::PluginHandle;
use tauri::{Emitter, Manager, Wry};

/// Managed state holding the Tauri mobile plugin handle for
/// `ConnectionServicePlugin` (Kotlin).
pub struct ConnectionServiceHandle(pub PluginHandle<Wry>);

/// Register a Tauri channel listener for the "disconnect-requested" event
/// emitted by the Kotlin `ConnectionServicePlugin` when the user taps the
/// "Disconnect" action on the foreground-service notification.
///
/// The channel callback spawns an async task to call `AppState::disconnect()`.
pub fn register_disconnect_listener(handle: &ConnectionServiceHandle, app: tauri::AppHandle) {
    let channel: tauri::ipc::Channel<tauri::ipc::InvokeBody> = tauri::ipc::Channel::new(move |_body| {
        let app = app.clone();
        let _ = tauri::async_runtime::spawn(async move {
            if let Some(state) = app.try_state::<AppState>() {
                if let Err(e) = state.disconnect().await {
                    tracing::warn!("notification disconnect failed: {e}");
                }
            }
        });
        Ok(())
    });

    let payload = serde_json::json!({
        "event": "disconnect-requested",
        "handler": channel,
    });
    if let Err(e) = handle.0.run_mobile_plugin::<()>("registerListener", payload) {
        tracing::warn!("failed to register disconnect listener: {e}");
    }
}

/// Register a Tauri channel listener for the "navigate-to-channel" event
/// emitted by the Kotlin `ConnectionServicePlugin` when the user taps a
/// chat notification.
///
/// The channel callback emits a Tauri event that the frontend listens for
/// in order to switch to the correct channel.
pub fn register_navigate_listener(handle: &ConnectionServiceHandle, app: tauri::AppHandle) {
    let channel: tauri::ipc::Channel<tauri::ipc::InvokeBody> =
        tauri::ipc::Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(ref json_str) = body {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                    if let Some(channel_id) = val.get("channelId").and_then(|v| v.as_u64()) {
                        let _ = app.emit(
                            "navigate-to-channel",
                            serde_json::json!({ "channel_id": channel_id }),
                        );
                    }
                }
            }
            Ok(())
        });

    let payload = serde_json::json!({
        "event": "navigate-to-channel",
        "handler": channel,
    });
    if let Err(e) = handle.0.run_mobile_plugin::<()>("registerListener", payload) {
        tracing::warn!("failed to register navigate listener: {e}");
    }
}

/// Start the foreground service, showing a persistent notification
/// saying "Connected to <server_name>".
pub fn start_service(handle: &ConnectionServiceHandle, server_name: &str) {
    tracing::info!(server_name, "starting connection service");
    let payload = serde_json::json!({ "serverName": server_name });
    if let Err(e) = handle.0.run_mobile_plugin::<()>("startService", payload) {
        tracing::warn!("failed to start connection service: {e}");
    }
}

/// Update the foreground-service notification text to include the
/// current channel name alongside the server name.
pub fn update_service_channel(
    handle: &ConnectionServiceHandle,
    server_name: &str,
    channel_name: &str,
) {
    tracing::info!(server_name, channel_name, "updating service channel notification");
    let payload =
        serde_json::json!({ "serverName": server_name, "channelName": channel_name });
    if let Err(e) = handle.0.run_mobile_plugin::<()>("updateServiceChannel", payload) {
        tracing::warn!("failed to update connection service notification: {e}");
    }
}

/// Stop the foreground service (called on disconnect).
pub fn stop_service(handle: &ConnectionServiceHandle) {
    if let Err(e) = handle.0.run_mobile_plugin::<()>("stopService", serde_json::json!({})) {
        tracing::warn!("failed to stop connection service: {e}");
    }
}

/// Show a chat-message notification with an optional sender avatar.
///
/// `icon_bytes` should be raw PNG or JPEG data (other formats are silently
/// ignored).  The bytes are Base64-encoded and decoded on the Kotlin side so
/// Android can render them as a `Bitmap` large-icon.
///
/// `channel_id` is attached to the notification intent so tapping the
/// notification navigates to the correct channel.
pub fn show_chat_notification(
    handle: &ConnectionServiceHandle,
    title: &str,
    body: &str,
    icon_bytes: Option<&[u8]>,
    channel_id: Option<u32>,
) {
    use base64::Engine;
    // Only base64-encode PNG or JPEG; skip the old Mumble raw-BGRA format.
    let icon_b64 = icon_bytes
        .filter(|b| b.starts_with(b"\x89PNG") || b.starts_with(b"\xff\xd8"))
        .map(|b| base64::engine::general_purpose::STANDARD.encode(b));
    let payload = serde_json::json!({
        "title": title,
        "body": body,
        "iconBase64": icon_b64,
        "channelId": channel_id,
    });
    if let Err(e) = handle.0.run_mobile_plugin::<()>("showChatNotification", payload) {
        tracing::warn!("failed to show chat notification: {e}");
    }
}
