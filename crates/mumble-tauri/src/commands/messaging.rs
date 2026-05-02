//! Channel chat message commands: read/write/edit, reactions, pins,
//! deletes, search, photos, typing/read receipts and link previews.

use crate::state::{self, AppState, ChatMessage, PhotoEntry, SearchResult};
use crate::state::protocol_commands::WatchSyncEventArg;

#[tauri::command]
pub(crate) fn super_search(
    state: tauri::State<'_, AppState>,
    query: String,
    filter: Option<state::types::SearchFilter>,
    channel_id: Option<u32>,
) -> Vec<SearchResult> {
    state.super_search(&query, filter.unwrap_or(state::types::SearchFilter::All), channel_id)
}

#[tauri::command]
pub(crate) fn get_photos(
    state: tauri::State<'_, AppState>,
    offset: usize,
    limit: usize,
) -> Vec<PhotoEntry> {
    state.get_photos(offset, limit)
}

#[tauri::command]
pub(crate) fn get_messages(state: tauri::State<'_, AppState>, channel_id: u32) -> Vec<ChatMessage> {
    state.messages(channel_id)
}

#[tauri::command]
pub(crate) async fn send_message(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    body: String,
) -> Result<(), String> {
    state.send_message(channel_id, body).await
}

#[tauri::command]
pub(crate) async fn edit_message(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    message_id: String,
    new_body: String,
) -> Result<(), String> {
    state.edit_message(channel_id, message_id, new_body).await
}

/// Notify the server that we are typing in a channel.
#[tauri::command]
pub(crate) async fn send_typing_indicator(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    tracing::debug!(channel_id, "send_typing_indicator invoked");
    state.send_typing_indicator(channel_id).await
}

/// Send a single watch-together (`FancyWatchSync`) event.
#[tauri::command]
pub(crate) async fn send_watch_sync(
    state: tauri::State<'_, AppState>,
    session_id: String,
    event: WatchSyncEventArg,
) -> Result<(), String> {
    state.send_watch_sync(session_id, event).await
}

/// Request link previews for the given URLs from the server.
#[tauri::command]
pub(crate) async fn request_link_preview(
    state: tauri::State<'_, AppState>,
    urls: Vec<String>,
    request_id: String,
) -> Result<(), String> {
    state.request_link_preview(urls, request_id).await
}

/// Send a read receipt watermark to the server.
#[tauri::command]
pub(crate) async fn send_read_receipt(
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
pub(crate) async fn query_read_receipts(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    state.query_read_receipts(channel_id).await
}

/// Send a reaction (add/remove) on a persisted chat message.
#[tauri::command]
pub(crate) async fn send_reaction(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    message_id: String,
    emoji: String,
    action: String,
) -> Result<(), String> {
    state.send_reaction(channel_id, message_id, emoji, action).await
}

/// Pin or unpin a persisted chat message.
#[tauri::command]
pub(crate) async fn pin_message(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    message_id: String,
    unpin: bool,
) -> Result<(), String> {
    state.pin_message(channel_id, message_id, unpin).await
}

/// Delete persisted chat messages on the server.
///
/// At least one of `message_ids`, `time_from`/`time_to`, or `sender_hash`
/// must be provided.
#[tauri::command]
pub(crate) async fn delete_pchat_messages(
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
