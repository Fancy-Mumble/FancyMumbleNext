//! Connection lifecycle commands.

use crate::state::{AppState, ConnectionStatus};

#[tauri::command]
pub(crate) async fn connect(
    state: tauri::State<'_, AppState>,
    host: String,
    port: u16,
    username: String,
    cert_label: Option<String>,
    password: Option<String>,
) -> Result<(), String> {
    state.connect(host, port, username, cert_label, password).await
}

#[tauri::command]
pub(crate) async fn disconnect(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.disconnect().await
}

#[tauri::command]
pub(crate) fn get_status(state: tauri::State<'_, AppState>) -> ConnectionStatus {
    state.status()
}
