//! Cross-cutting system commands: notifications, log level, badge count,
//! factory reset and clock format.

use crate::platform;
use crate::state::AppState;
use crate::LOG_RELOAD_HANDLE;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

/// Returns the OS-detected clock format for the "auto" time setting.
#[tauri::command]
pub(crate) fn get_system_clock_format() -> Option<&'static str> {
    platform::badge::system_clock_format()
}

/// Enable or disable native OS notifications.
#[tauri::command]
pub(crate) fn set_notifications_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    state.inner.snapshot().lock().map_err(|e| e.to_string())?.prefs.notifications_enabled = enabled;
    Ok(())
}

/// Enable or disable dual-path sending for encrypted channels.
///
/// When disabled, the plain `TextMessage` body is replaced with a
/// placeholder so the server never sees the cleartext content.
#[tauri::command]
pub(crate) fn set_disable_dual_path(
    state: tauri::State<'_, AppState>,
    disabled: bool,
) -> Result<(), String> {
    state.inner.snapshot().lock().map_err(|e| e.to_string())?.prefs.disable_dual_path = disabled;
    Ok(())
}

/// Change the log level filter at runtime.
///
/// Accepts a `tracing_subscriber::EnvFilter`-compatible string such as
/// `"debug"`, `"mumble_tauri=debug,mumble_protocol=debug,info"`, or
/// `"trace"`.  Returns the filter that was actually applied.
#[tauri::command]
pub(crate) fn set_log_level(filter: String) -> Result<String, String> {
    let handle = LOG_RELOAD_HANDLE
        .get()
        .ok_or_else(|| "logging not initialised".to_string())?;
    let new_filter =
        EnvFilter::try_new(&filter).map_err(|e| format!("invalid filter '{filter}': {e}"))?;
    let applied = format!("{new_filter}");
    handle
        .reload(new_filter)
        .map_err(|e| format!("failed to reload filter: {e}"))?;
    tracing::info!(filter = %applied, "log level changed");
    Ok(applied)
}

/// Reset all app data to factory defaults (preferences, saved servers, certs).
#[tauri::command]
pub(crate) async fn reset_app_data(app: tauri::AppHandle) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    // Remove known data files.
    for name in &["preferences.json", "servers.json", "passwords.json"] {
        let path = data_dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    // Remove certs directory.
    let certs = data_dir.join("certs");
    if certs.exists() {
        std::fs::remove_dir_all(&certs).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Set the taskbar badge count.
///
/// On Windows this renders a small red overlay icon with the count (the native
/// `set_badge_count` API is not supported). On Linux/macOS it delegates to
/// the native badge-count API. On Android/iOS this is a no-op.
#[tauri::command]
pub(crate) fn update_badge_count(window: tauri::Window, count: Option<u32>) -> Result<(), String> {
    platform::badge::set_badge(&window, count)
}
