//! GNOME / Linux desktop integration.
//!
//! - Installs a `.desktop` file so GNOME shows the correct app name and icon.
//! - Installs the app icon into the user icon theme.
//! - Provides quick-action IPC via a Unix domain socket so `.desktop` actions
//!   (Mute, Deafen, Disconnect) can reach the running instance.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use tauri::Manager;
use tracing::{info, warn};

use crate::state::AppState;

/// XDG base directory for application launchers.
const APPLICATIONS_DIR: &str = "applications";
/// XDG base directory for icons (hicolor theme, 256x256).
const ICON_SUBDIR: &str = "icons/hicolor/256x256/apps";
/// Desktop file name (must match the GTK application ID).
const DESKTOP_FILE_NAME: &str = "com.fancymumble.app.desktop";
/// Icon name (without extension) referenced by the desktop file.
const ICON_NAME: &str = "com.fancymumble.app";

/// Socket file name placed inside `$XDG_RUNTIME_DIR`.
const SOCKET_NAME: &str = "com.fancymumble.app.sock";

// -- Desktop file template ------------------------------------------------

/// Render the `.desktop` file contents.
///
/// `exec_path` is substituted into the `Exec` lines so that quick actions
/// work regardless of where the binary is installed.
fn desktop_file_contents(exec_path: &str) -> String {
    format!(
        "\
[Desktop Entry]
Type=Application
Name=Fancy Mumble
GenericName=Mumble Client
Comment=Modern Mumble voice chat client
Exec={exec_path} %U
Icon={ICON_NAME}
Terminal=false
StartupWMClass=com.fancymumble.app
Categories=Network;Chat;AudioVideo;
Keywords=mumble;voip;voice;chat;
Actions=mute;deafen;disconnect;

[Desktop Action mute]
Name=Toggle Mute
Exec={exec_path} --action mute

[Desktop Action deafen]
Name=Toggle Deafen
Exec={exec_path} --action deafen

[Desktop Action disconnect]
Name=Disconnect
Exec={exec_path} --action disconnect
"
    )
}

// -- XDG helpers ----------------------------------------------------------

/// `$XDG_DATA_HOME` or `~/.local/share`.
fn xdg_data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs_fallback("/.local/share"))
}

/// `$XDG_RUNTIME_DIR` or `/run/user/<uid>`.
fn xdg_runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Fallback that appends `suffix` to `$HOME`.
fn dirs_fallback(suffix: &str) -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| {
        let mut p = PathBuf::from(h);
        p.push(suffix.trim_start_matches('/'));
        p
    })
}

// -- Desktop file & icon installation -------------------------------------

/// Install (or update) the `.desktop` file and the app icon.
///
/// Called once during `setup`.  Files are written to the per-user XDG
/// directories so no root privileges are required.
pub fn install_desktop_entry() {
    let Some(data_home) = xdg_data_home() else {
        warn!("Could not determine XDG_DATA_HOME; skipping desktop file install");
        return;
    };

    // Resolve the path to the running binary for Exec lines.
    let exec_path = match std::env::current_exe().and_then(std::fs::canonicalize) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(e) => {
            warn!("Could not determine executable path: {e}");
            // Fall back to bare binary name; it must be on $PATH.
            "mumble-tauri".to_string()
        }
    };

    // -- .desktop file ---------------------------------------------------
    let apps_dir = data_home.join(APPLICATIONS_DIR);
    if let Err(e) = std::fs::create_dir_all(&apps_dir) {
        warn!("Failed to create {}: {e}", apps_dir.display());
        return;
    }

    let desktop_path = apps_dir.join(DESKTOP_FILE_NAME);
    let contents = desktop_file_contents(&exec_path);

    // Only write when changed (avoids unnecessary inotify churn).
    let needs_write = std::fs::read_to_string(&desktop_path)
        .map(|existing| existing != contents)
        .unwrap_or(true);

    if needs_write {
        match std::fs::write(&desktop_path, &contents) {
            Ok(()) => info!("Installed desktop file: {}", desktop_path.display()),
            Err(e) => warn!("Failed to write desktop file: {e}"),
        }
    }

    // -- Icon ------------------------------------------------------------
    let icon_dir = data_home.join(ICON_SUBDIR);
    if let Err(e) = std::fs::create_dir_all(&icon_dir) {
        warn!("Failed to create {}: {e}", icon_dir.display());
        return;
    }
    let icon_dest = icon_dir.join(format!("{ICON_NAME}.png"));

    // Ship the icon embedded in the binary so it works in dev mode too.
    let icon_bytes = include_bytes!("../../../icons/icon.png");

    let needs_icon = std::fs::metadata(&icon_dest)
        .map(|m| m.len() != icon_bytes.len() as u64)
        .unwrap_or(true);

    if needs_icon {
        match std::fs::write(&icon_dest, icon_bytes) {
            Ok(()) => info!("Installed app icon: {}", icon_dest.display()),
            Err(e) => warn!("Failed to write app icon: {e}"),
        }
    }
}

// -- GTK prgname ----------------------------------------------------------

/// Set the GTK program name and application name so that GNOME can match
/// the running window to the `.desktop` file on both X11 and Wayland.
///
/// Must be called **before** GTK is initialised (i.e. before
/// `tauri::Builder::build`).
pub fn set_gtk_identifiers() {
    // Safety: g_set_prgname / g_set_application_name are thread-safe glib
    // functions that take a NUL-terminated C string.  We call them before
    // any GTK/GLib threads are spawned.
    #[allow(unsafe_code, reason = "calling well-defined glib C API before GTK init")]
    {
        use std::ffi::CString;
        extern "C" {
            fn g_set_prgname(prgname: *const std::ffi::c_char);
            fn g_set_application_name(name: *const std::ffi::c_char);
        }
        if let (Ok(prgname), Ok(appname)) = (
            CString::new("com.fancymumble.app"),
            CString::new("Fancy Mumble"),
        ) {
            // SAFETY: Both pointers are valid NUL-terminated strings whose
            // lifetime extends beyond the call.  glib copies them internally.
            unsafe {
                g_set_prgname(prgname.as_ptr());
                g_set_application_name(appname.as_ptr());
            }
        }
    }
}

// -- Quick-action IPC (Unix domain socket) --------------------------------

/// Actions that can be triggered from `.desktop` file quick-actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickAction {
    Mute,
    Deafen,
    Disconnect,
}

impl QuickAction {
    fn from_str(s: &str) -> Option<Self> {
        match s.trim() {
            "mute" => Some(Self::Mute),
            "deafen" => Some(Self::Deafen),
            "disconnect" => Some(Self::Disconnect),
            _ => None,
        }
    }
}

/// Path to the IPC socket.
fn socket_path() -> PathBuf {
    xdg_runtime_dir().join(SOCKET_NAME)
}

/// Check CLI args for `--action <name>`.
///
/// If found, send the action to the running instance via the Unix socket
/// and return `true` (the caller should exit).  Returns `false` if no
/// action arg is present.
pub fn try_send_quick_action() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(idx) = args.iter().position(|a| a == "--action") else {
        return false;
    };
    let Some(action_str) = args.get(idx + 1) else {
        eprintln!("--action requires a value (mute, deafen, disconnect)");
        return true;
    };

    if QuickAction::from_str(action_str).is_none() {
        eprintln!("Unknown action: {action_str}");
        return true;
    }

    let path = socket_path();
    match UnixStream::connect(&path) {
        Ok(mut stream) => {
            let _ = stream.write_all(action_str.trim().as_bytes());
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        Err(e) => {
            eprintln!("Could not connect to running Fancy Mumble instance: {e}");
        }
    }
    true
}

/// Read a single incoming connection and dispatch the action it carries.
///
/// Returns `true` to keep listening, `false` to stop.
fn handle_incoming(
    app_handle: &tauri::AppHandle,
    stream: std::io::Result<UnixStream>,
) -> bool {
    let mut s = match stream {
        Ok(s) => s,
        Err(e) => {
            warn!("Quick-action listener error: {e}");
            return false;
        }
    };
    let mut buf = String::with_capacity(16);
    let action = s
        .read_to_string(&mut buf)
        .ok()
        .and_then(|_| QuickAction::from_str(&buf));
    if let Some(action) = action {
        dispatch_action(app_handle, action);
    }
    true
}

/// Start the background listener that receives quick-action commands
/// from secondary process invocations.
///
/// Spawns a dedicated OS thread (not a tokio task) so the blocking
/// `accept()` loop doesn't consume an async worker.
pub fn start_action_listener(app_handle: tauri::AppHandle) {
    let path = socket_path();

    // Remove stale socket from a previous run.
    let _ = std::fs::remove_file(&path);

    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            warn!("Failed to bind quick-action socket {}: {e}", path.display());
            return;
        }
    };

    info!("Quick-action listener started on {}", path.display());

    drop(
        std::thread::Builder::new()
            .name("gnome-action-listener".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if !handle_incoming(&app_handle, stream) {
                        break;
                    }
                }
            }),
    );
}

/// Execute a quick action on the running `AppState`.
fn dispatch_action(app_handle: &tauri::AppHandle, action: QuickAction) {
    let handle = app_handle.clone();

    // The state methods are async, so spawn a tokio task.
    drop(tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let result = match action {
            QuickAction::Mute => {
                info!("Quick action: toggle mute");
                state.toggle_mute().await
            }
            QuickAction::Deafen => {
                info!("Quick action: toggle deafen");
                state.toggle_deafen().await
            }
            QuickAction::Disconnect => {
                info!("Quick action: disconnect");
                state.disconnect().await
            }
        };

        if let Err(e) = result {
            warn!("Quick action {action:?} failed: {e}");
        }

        // Bring the window to front so the user sees the effect.
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.set_focus();
        }
    }));
}

/// Clean up the IPC socket file on shutdown.
pub fn cleanup_socket() {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_action_from_str_parses_valid_actions() {
        assert_eq!(QuickAction::from_str("mute"), Some(QuickAction::Mute));
        assert_eq!(QuickAction::from_str("deafen"), Some(QuickAction::Deafen));
        assert_eq!(
            QuickAction::from_str("disconnect"),
            Some(QuickAction::Disconnect)
        );
    }

    #[test]
    fn quick_action_from_str_rejects_invalid() {
        assert_eq!(QuickAction::from_str(""), None);
        assert_eq!(QuickAction::from_str("unknown"), None);
    }

    #[test]
    fn desktop_file_contains_required_fields() {
        let content = desktop_file_contents("/usr/bin/fancy-mumble");
        assert!(content.contains("Name=Fancy Mumble"));
        assert!(content.contains("Exec=/usr/bin/fancy-mumble %U"));
        assert!(content.contains("[Desktop Action mute]"));
        assert!(content.contains("[Desktop Action deafen]"));
        assert!(content.contains("[Desktop Action disconnect]"));
        assert!(content.contains("StartupWMClass=com.fancymumble.app"));
        assert!(content.contains("Icon=com.fancymumble.app"));
        assert!(content.contains("Actions=mute;deafen;disconnect;"));
    }

    #[test]
    fn xdg_data_home_returns_something() {
        // Should always resolve on Linux (either XDG_DATA_HOME or $HOME/.local/share).
        assert!(xdg_data_home().is_some());
    }
}
