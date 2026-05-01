//! Multi-server session commands.

use crate::state::{AppState, ServerId, SessionMeta};

#[tauri::command]
pub(crate) fn list_servers(state: tauri::State<'_, AppState>) -> Vec<SessionMeta> {
    state.registry.list_meta()
}

#[tauri::command]
pub(crate) fn get_active_server(state: tauri::State<'_, AppState>) -> Option<ServerId> {
    state.registry.active_id()
}

#[tauri::command]
pub(crate) async fn set_active_server(
    state: tauri::State<'_, AppState>,
    server_id: ServerId,
) -> Result<(), String> {
    state.switch_active_with_voice(server_id).await
}

/// Disconnect a specific session by id.  Operates only on that
/// session's connection / state — does not touch the active session's
/// `inner` pointer or its audio pipeline (unless `server_id` itself
/// is the active session).
#[tauri::command]
pub(crate) async fn disconnect_server(
    state: tauri::State<'_, AppState>,
    server_id: ServerId,
) -> Result<(), String> {
    state.disconnect_session(server_id).await
}
