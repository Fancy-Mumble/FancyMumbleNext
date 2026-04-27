//! Content-offloading and pagination commands.

use std::collections::HashMap;

use crate::state::{AppState, DebugStats};

/// Encrypt a heavy message body and write it to a temp file, replacing
/// the in-memory body with a lightweight placeholder.
///
/// `scope` is `"channel"`, `"dm"`, or `"group"`.
/// `scope_id` is the channel ID, DM session, or group UUID as a string.
#[tauri::command]
pub(crate) fn offload_message(
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
pub(crate) fn load_offloaded_message(
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
pub(crate) fn load_offloaded_messages_batch(
    state: tauri::State<'_, AppState>,
    message_ids: Vec<String>,
    scope: String,
    scope_id: String,
) -> Result<HashMap<String, String>, String> {
    state.load_offloaded_messages_batch(message_ids, scope, scope_id)
}

/// Delete all offloaded temp files.
#[tauri::command]
pub(crate) fn clear_offloaded_messages(state: tauri::State<'_, AppState>) {
    state.clear_offloaded();
}

/// Send a `PchatFetch` request for older messages (pagination).
/// The response arrives asynchronously via the `PchatFetchResponse` handler
/// which emits `"pchat-fetch-complete"` and `"new-message"` events.
#[tauri::command]
pub(crate) async fn fetch_older_messages(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    before_id: Option<String>,
    limit: u32,
) -> Result<(), String> {
    state.fetch_older_messages(channel_id, before_id, limit).await
}

/// Collect debug statistics for the developer info panel.
#[tauri::command]
pub(crate) fn get_debug_stats(state: tauri::State<'_, AppState>) -> DebugStats {
    state.debug_stats()
}
