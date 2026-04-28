//! Tauri commands exposed to the bootstrapper window.

use super::manager::{UpdateInfo, UpdaterState};
use super::window;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

/// Event channel for download progress, emitted to the updater window only.
const PROGRESS_EVENT: &str = "updater://progress";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProgressEvent {
    Started { total: Option<u64> },
    Chunk { downloaded: u64, total: Option<u64> },
    Finished,
}

/// Internal helper: perform an update check and stash the result.
///
/// Returns `true` when an update is available *and* not on the user's
/// skip list. A skipped version is treated identically to "no update".
pub(super) async fn run_check(app: &tauri::AppHandle) -> Result<bool, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await.map_err(|e| e.to_string())? {
        Some(update) => {
            let skipped = app
                .try_state::<UpdaterState>()
                .and_then(|s| s.skipped_version());
            if skipped.as_deref() == Some(update.version.as_str()) {
                tracing::info!(
                    "Updater: version {} is on the user's skip list, ignoring",
                    update.version
                );
                return Ok(false);
            }
            if let Some(state) = app.try_state::<UpdaterState>() {
                state.store(update);
            }
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Force a fresh update check from the bootstrapper UI.
#[tauri::command]
pub(crate) async fn updater_check(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    let _ = run_check(&app).await?;
    Ok(app.try_state::<UpdaterState>().and_then(|s| s.snapshot()))
}

/// Return the cached update info without triggering a new check.
#[tauri::command]
pub(crate) fn updater_pending(state: tauri::State<'_, UpdaterState>) -> Option<UpdateInfo> {
    state.snapshot()
}

/// Download and install the cached update, emitting progress events to
/// the updater window. On Windows the app exits before the installer
/// runs; on macOS / Linux the bootstrapper UI is responsible for calling
/// `relaunch()` once this command resolves.
#[tauri::command]
pub(crate) async fn updater_download_and_install(
    app: tauri::AppHandle,
    state: tauri::State<'_, UpdaterState>,
) -> Result<(), String> {
    if cfg!(debug_assertions) {
        tracing::warn!("Updater: skipping install in debug/dev build (simulating progress)");
        const FAKE_TOTAL: u64 = 1_000_000;
        const STEPS: u64 = 10;
        let total: Option<u64> = Some(FAKE_TOTAL);
        emit_progress(&app, ProgressEvent::Started { total });
        for i in 1..=STEPS {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let downloaded = FAKE_TOTAL * i / STEPS;
            emit_progress(&app, ProgressEvent::Chunk { downloaded, total });
        }
        emit_progress(&app, ProgressEvent::Finished);
        return Ok(());
    }
    let update = state.take().ok_or_else(|| "no pending update".to_string())?;

    let mut total: Option<u64> = None;
    let mut downloaded: u64 = 0;
    let app_for_progress = app.clone();

    update
        .download_and_install(
            move |chunk_len, content_len| {
                if total.is_none() {
                    total = content_len;
                    emit_progress(&app_for_progress, ProgressEvent::Started { total });
                }
                downloaded = downloaded.saturating_add(chunk_len as u64);
                emit_progress(
                    &app_for_progress,
                    ProgressEvent::Chunk { downloaded, total },
                );
            },
            move || {},
        )
        .await
        .map_err(|e| e.to_string())?;

    emit_progress(&app, ProgressEvent::Finished);
    Ok(())
}

/// Close the updater window without installing.
#[tauri::command]
pub(crate) fn updater_dismiss(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(window::UPDATER_WINDOW_LABEL) {
        win.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Persist the user's auto-install preference into the in-process
/// updater state. Called by the main webview during startup.
#[tauri::command]
pub(crate) fn updater_set_auto_install(
    enabled: bool,
    state: tauri::State<'_, UpdaterState>,
) {
    state.set_auto_install(enabled);
}

/// Persist the user's "skip this version" choice into the in-process
/// updater state. Pass `None` to clear it.
#[tauri::command]
pub(crate) fn updater_set_skipped_version(
    version: Option<String>,
    state: tauri::State<'_, UpdaterState>,
) {
    state.set_skipped_version(version);
}

fn emit_progress(app: &tauri::AppHandle, event: ProgressEvent) {
    if let Some(win) = app.get_webview_window(window::UPDATER_WINDOW_LABEL) {
        let _ = win.emit(PROGRESS_EVENT, event);
    }
}
