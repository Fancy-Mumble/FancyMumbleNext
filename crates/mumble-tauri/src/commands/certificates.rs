//! Identity (TLS client certificate) management commands.

use tauri::Manager;

use crate::state;

/// Generate a self-signed TLS client certificate for an identity label.
/// Each identity gets its own folder under `{app_data}/identities/{label}/`
/// containing both the TLS cert and the pchat seed.
/// Does nothing if the certificate already exists.
#[tauri::command]
pub(crate) async fn generate_certificate(
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    state::pchat::IdentityStore::new(data_dir).generate_cert(&label)
}

/// List the labels of all identities stored in `{app_data_dir}/identities/`.
#[tauri::command]
pub(crate) async fn list_certificates(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    Ok(state::pchat::IdentityStore::new(data_dir).list_labels())
}

/// Delete an identity (TLS cert + pchat seed) by label.
#[tauri::command]
pub(crate) async fn delete_certificate(
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    state::pchat::IdentityStore::new(data_dir).delete(&label)
}

/// Export an identity to a user-chosen file via the native save dialog.
#[tauri::command]
pub(crate) async fn export_certificate(
    app: tauri::AppHandle,
    label: String,
    dest_path: String,
) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    state::pchat::IdentityStore::new(data_dir).export(&label, std::path::Path::new(&dest_path))
}

/// Import an identity from a user-chosen file via the native open dialog.
/// Returns the label of the imported identity.
#[tauri::command]
pub(crate) async fn import_certificate(
    app: tauri::AppHandle,
    src_path: String,
) -> Result<String, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    state::pchat::IdentityStore::new(data_dir).import(std::path::Path::new(&src_path))
}
