//! Direct-message conversation commands.

use std::collections::HashMap;

use crate::state::{AppState, ChatMessage};

/// Send a direct message to a specific user.
#[tauri::command]
pub(crate) async fn send_dm(
    state: tauri::State<'_, AppState>,
    target_session: u32,
    body: String,
) -> Result<(), String> {
    state.send_dm(target_session, body).await
}

/// Get DM messages for a conversation with a specific user.
#[tauri::command]
pub(crate) fn get_dm_messages(state: tauri::State<'_, AppState>, session: u32) -> Vec<ChatMessage> {
    state.dm_messages(session)
}

/// Select a DM conversation for viewing.
#[tauri::command]
pub(crate) fn select_dm_user(state: tauri::State<'_, AppState>, session: u32) -> Result<(), String> {
    state.select_dm_user(session)
}

/// Get DM unread counts per user session.
#[tauri::command]
pub(crate) fn get_dm_unread_counts(state: tauri::State<'_, AppState>) -> HashMap<u32, u32> {
    state.dm_unread_counts()
}

/// Mark DMs with a specific user as read.
#[tauri::command]
pub(crate) fn mark_dm_read(state: tauri::State<'_, AppState>, session: u32) {
    state.mark_dm_read(session);
}
