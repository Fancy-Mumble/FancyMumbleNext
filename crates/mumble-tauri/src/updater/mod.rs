//! Self-contained Tauri auto-updater integration.
//!
//! Everything related to checking for, downloading and installing
//! application updates lives in this module. The rest of the app only
//! interacts with it through:
//!
//! * [`register_plugins`] - call once when building the [`tauri::Builder`].
//! * [`commands`] - re-exported `#[tauri::command]` handlers to register.
//! * [`spawn_startup_check`] - kicks off a background update check on launch
//!   and opens the branded bootstrapper window when an update is available.
//!
//! The branded bootstrapper UI lives in `ui/src/updater/` and is loaded
//! into a dedicated [`tauri::WebviewWindow`] with the label
//! [`UPDATER_WINDOW_LABEL`].

#![cfg(not(target_os = "android"))]

pub(crate) mod commands;
mod manager;
mod window;

pub(crate) use manager::UpdaterState;
pub(crate) use window::{show_main_window, MAIN_WINDOW_LABEL, UPDATER_WINDOW_LABEL};

use tauri::{Manager, Wry};

/// Register the `updater` and `process` Tauri plugins on the builder.
///
/// `process` is needed so the bootstrapper UI can call `relaunch()`
/// after a successful update on macOS / Linux (Windows relaunches
/// automatically as part of the installer flow).
pub(crate) fn register_plugins(builder: tauri::Builder<Wry>) -> tauri::Builder<Wry> {
    builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
}

/// Install the shared [`UpdaterState`] and kick off the background
/// check-on-startup task. Safe to call once from the main `setup` hook.
pub(crate) fn init(app: &tauri::AppHandle) {
    let _ = app.manage(UpdaterState::default());
    load_persisted_prefs(app);
    spawn_startup_check(app.clone());
}

/// Read `preferences.json` (written by `@tauri-apps/plugin-store`) and
/// hydrate the [`UpdaterState`] before the startup check runs. This
/// avoids a race where the JS in the main webview hasn't yet pushed
/// the user's preferences via the `updater_set_*` commands by the time
/// `spawn_startup_check` decides whether to auto-install.
fn load_persisted_prefs(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<UpdaterState>() else {
        return;
    };
    let Ok(config_dir) = app.path().app_config_dir() else {
        return;
    };
    let path = config_dir.join("preferences.json");
    let Ok(bytes) = std::fs::read(&path) else {
        tracing::debug!("Updater: no persisted preferences at {}", path.display());
        return;
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        tracing::warn!("Updater: preferences.json is not valid JSON");
        return;
    };
    let prefs = json.get("preferences").unwrap_or(&json);
    if let Some(b) = prefs.get("autoUpdateOnStartup").and_then(|v| v.as_bool()) {
        state.set_auto_install(b);
        tracing::info!("Updater: auto-install on startup = {b}");
    }
    if let Some(v) = prefs.get("skippedUpdateVersion").and_then(|v| v.as_str()) {
        state.set_skipped_version(Some(v.to_string()));
    }
}

/// Spawn an async task that checks for updates shortly after launch.
///
/// * If an update is available and not skipped: open the branded
///   bootstrapper window and keep the main window hidden.
/// * Otherwise: reveal the main window immediately.
fn spawn_startup_check(app: tauri::AppHandle) {
    drop(tauri::async_runtime::spawn(async move {
        // Tiny delay so the main webview has a chance to register its
        // `updater_set_auto_install` and `updater_set_skipped_version`
        // preferences before we open the window.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        match commands::run_check(&app).await {
            Ok(true) => {
                let auto = app
                    .try_state::<UpdaterState>()
                    .map(|s| s.auto_install())
                    .unwrap_or(false);
                if let Err(e) = window::open_updater_window(&app, auto) {
                    tracing::warn!("Failed to open updater window: {e}");
                    show_main_window(&app);
                }
            }
            Ok(false) => {
                tracing::debug!("Updater: no update available");
                show_main_window(&app);
            }
            Err(e) => {
                tracing::info!("Updater: startup check failed: {e}");
                show_main_window(&app);
            }
        }
    }));
}
