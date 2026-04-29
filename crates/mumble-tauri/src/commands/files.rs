//! File upload/download and custom emote management.

use crate::state::{
    AddEmoteRequest, AddEmoteResponse, AppState, DownloadRequest, RemoveEmoteRequest,
    UploadRequest, UploadResponse,
};

/// Upload a local file to the server-side `mumble-file-server` plugin and
/// return the signed download URL.
#[tauri::command]
pub(crate) async fn upload_file(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    request: UploadRequest,
) -> Result<UploadResponse, String> {
    state.upload_file(request, app_handle).await
}

/// Cancel an in-progress upload by its `upload_id`.
#[tauri::command]
pub(crate) fn cancel_upload(state: tauri::State<'_, AppState>, upload_id: String) {
    let _ = state.cancel_upload(&upload_id);
}

/// Download a file (optionally performing the password / session-JWT
/// pre-auth ticket exchange) and write it to disk. Returns the number
/// of bytes written.
#[tauri::command]
pub(crate) async fn download_file(
    state: tauri::State<'_, AppState>,
    request: DownloadRequest,
) -> Result<u64, String> {
    state.download_file(request).await
}

/// Upload a custom server emote (admin-only on the server side).
#[tauri::command]
pub(crate) async fn add_custom_emote(
    state: tauri::State<'_, AppState>,
    request: AddEmoteRequest,
) -> Result<AddEmoteResponse, String> {
    state.add_custom_emote(request).await
}

/// Delete a custom server emote (admin-only on the server side).
#[tauri::command]
pub(crate) async fn remove_custom_emote(
    state: tauri::State<'_, AppState>,
    request: RemoveEmoteRequest,
) -> Result<(), String> {
    state.remove_custom_emote(request).await
}
