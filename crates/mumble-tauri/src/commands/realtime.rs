//! Real-time auxiliary commands: push notifications, typing indicators,
//! link previews and WebRTC signalling.

use crate::state::AppState;

/// Update the per-channel push notification mute preferences on the server.
#[tauri::command]
pub(crate) async fn send_push_update(
    state: tauri::State<'_, AppState>,
    muted_channels: Vec<u32>,
) -> Result<(), String> {
    state.send_push_update(muted_channels).await
}

/// Send a live subscribe-push registration (or update) to the server.
#[tauri::command]
pub(crate) async fn send_subscribe_push(
    state: tauri::State<'_, AppState>,
    muted_channels: Vec<u32>,
) -> Result<(), String> {
    state.send_subscribe_push(muted_channels).await
}

/// Send a WebRTC screen-sharing signaling message.
///
/// `target_session` of 0 broadcasts to all channel members.
///
/// `server_id` (optional) selects which connection to send the signal
/// through.  Required when multiple server tabs are open in the same
/// window: trickling ICE candidates from a background tab's broadcaster
/// would otherwise be sent through the active tab's connection,
/// derailing the SFU handshake.  When `None`, the active session is
/// used (legacy callers, single-tab scenarios).
#[tauri::command]
pub(crate) async fn send_webrtc_signal(
    state: tauri::State<'_, AppState>,
    target_session: u32,
    signal_type: i32,
    payload: String,
    server_id: Option<String>,
) -> Result<(), String> {
    state
        .send_webrtc_signal(target_session, signal_type, payload, server_id)
        .await
}
