//! Own profile updates and arbitrary plugin-data transmission.

use crate::state::AppState;

/// Set the user comment on the connected server (`FancyMumble` profile + bio).
#[tauri::command]
pub(crate) async fn set_user_comment(
    state: tauri::State<'_, AppState>,
    comment: String,
) -> Result<(), String> {
    state.set_user_comment(comment).await
}

/// Set the user avatar texture on the connected server (raw image bytes).
///
/// Accepts a JSON array of `u8` values from the frontend.
#[tauri::command]
pub(crate) async fn set_user_texture(
    state: tauri::State<'_, AppState>,
    texture: Vec<u8>,
) -> Result<(), String> {
    state.set_user_texture(texture).await
}

/// Return the local user's session ID assigned by the server.
#[tauri::command]
pub(crate) fn get_own_session(state: tauri::State<'_, AppState>) -> Option<u32> {
    state.get_own_session()
}

/// Send a plugin data transmission (e.g. polls) to the server.
///
/// `receiver_sessions` can be empty to broadcast to all users.
#[tauri::command]
pub(crate) async fn send_plugin_data(
    state: tauri::State<'_, AppState>,
    receiver_sessions: Vec<u32>,
    data: Vec<u8>,
    data_id: String,
) -> Result<(), String> {
    state.send_plugin_data(receiver_sessions, data, data_id).await
}
