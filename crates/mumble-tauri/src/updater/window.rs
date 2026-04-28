//! Spawns the branded "bootstrapper" updater window.
//!
//! The window loads the same `index.html` as the main app, but with the
//! query string `?updater=1` so the React entry point routes to
//! `ui/src/updater/UpdaterWindow.tsx` instead of the regular app.

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

pub(crate) const UPDATER_WINDOW_LABEL: &str = "updater";
pub(crate) const MAIN_WINDOW_LABEL: &str = "main";

const UPDATER_WINDOW_WIDTH: f64 = 520.0;
const UPDATER_WINDOW_HEIGHT: f64 = 720.0;

/// Reveal the main app window, focusing it if necessary. The main
/// window starts hidden in `tauri.conf.json` so the updater can decide
/// whether the app should appear on launch.
pub(crate) fn show_main_window(app: &tauri::AppHandle) {
    let Some(win) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        tracing::warn!("show_main_window: main window not found");
        return;
    };
    if let Err(e) = win.show() {
        tracing::warn!("show_main_window: failed to show: {e}");
    }
    let _ = win.set_focus();
}

/// Open the updater window, focusing an existing one if already present.
///
/// When `auto_install` is true the window URL gets a `&auto=1` flag so
/// the React bootstrapper starts the download/install immediately.
pub(crate) fn open_updater_window(
    app: &tauri::AppHandle,
    auto_install: bool,
) -> tauri::Result<()> {
    if let Some(existing) = app.get_webview_window(UPDATER_WINDOW_LABEL) {
        let _ = existing.set_focus();
        return Ok(());
    }

    let url = if auto_install {
        "index.html?updater=1&auto=1"
    } else {
        "index.html?updater=1"
    };

    let _ = WebviewWindowBuilder::new(
        app,
        UPDATER_WINDOW_LABEL,
        WebviewUrl::App(url.into()),
    )
    .title("Fancy Mumble Updater")
    .inner_size(UPDATER_WINDOW_WIDTH, UPDATER_WINDOW_HEIGHT)
    .min_inner_size(UPDATER_WINDOW_WIDTH, UPDATER_WINDOW_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(true)
    .decorations(false)
    .transparent(false)
    .center()
    .always_on_top(false)
    .skip_taskbar(false)
    .build()?;
    Ok(())
}
