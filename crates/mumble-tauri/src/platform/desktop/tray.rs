//! System tray icon with voice-state indicator and quick actions.
//!
//! Shows the app icon in the GNOME top bar (or equivalent on other DEs).
//! The icon gains a green ring overlay while the user is transmitting audio.
//! Right-click menu provides Mute, Deafen, Disconnect, Show Window, and Quit.

use std::sync::OnceLock;

use tauri::image::Image;
use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItem, MenuItemBuilder,
    PredefinedMenuItem,
};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager};
use tracing::{info, warn};

use crate::state::AppState;

/// Embedded app icon (compile-time).
const ICON_BYTES: &[u8] = include_bytes!("../../../icons/icon.png");

/// Tray icon ID so we can look it up later.
const TRAY_ID: &str = "main-tray";

// -- Menu item IDs --------------------------------------------------------

const ID_MUTE: &str = "tray-mute";
const ID_DEAFEN: &str = "tray-deafen";
const ID_DISCONNECT: &str = "tray-disconnect";
const ID_SHOW: &str = "tray-show";
const ID_QUIT: &str = "tray-quit";

/// Stored handles to menu items so we can update their state at runtime.
static MUTE_ITEM: OnceLock<CheckMenuItem<tauri::Wry>> = OnceLock::new();
static DEAFEN_ITEM: OnceLock<CheckMenuItem<tauri::Wry>> = OnceLock::new();
static DISCONNECT_ITEM: OnceLock<MenuItem<tauri::Wry>> = OnceLock::new();

// -- Public API -----------------------------------------------------------

/// Create the system tray icon with context menu.
///
/// Call this inside `.setup()` after `AppState` has been managed.
pub fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let toggle_mute = CheckMenuItemBuilder::with_id(ID_MUTE, "Mute")
        .enabled(false)
        .build(app)?;
    let toggle_deafen = CheckMenuItemBuilder::with_id(ID_DEAFEN, "Deafen")
        .enabled(false)
        .build(app)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let disconnect = MenuItemBuilder::with_id(ID_DISCONNECT, "Disconnect")
        .enabled(false)
        .build(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let show_window = MenuItemBuilder::with_id(ID_SHOW, "Show Window").build(app)?;
    let quit = MenuItemBuilder::with_id(ID_QUIT, "Quit").build(app)?;

    // Store handles for later state sync.
    let _ = MUTE_ITEM.set(toggle_mute.clone());
    let _ = DEAFEN_ITEM.set(toggle_deafen.clone());
    let _ = DISCONNECT_ITEM.set(disconnect.clone());

    let menu = MenuBuilder::new(app)
        .items(&[
            &toggle_mute,
            &toggle_deafen,
            &sep1,
            &disconnect,
            &sep2,
            &show_window,
            &quit,
        ])
        .build()?;

    let icon = Image::from_bytes(ICON_BYTES)?;

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Fancy Mumble")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            // Left-click: show and focus the main window.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                if let Some(w) = tray.app_handle().get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;

    // Listen for connection events to enable/disable server-dependent items.
    let handle_conn = app.handle().clone();
    let _ = app.listen("server-connected", move |_event| {
        set_connection_items_enabled(true);
        sync_tray_checks(&handle_conn);
    });
    let handle_disc = app.handle().clone();
    let _ = app.listen("server-disconnected", move |_event| {
        set_connection_items_enabled(false);
        // Reset icon to normal on disconnect.
        update_tray_icon(&handle_disc, false);
    });

    // Listen for voice-state changes to sync the Mute/Deafen checkmarks.
    let handle = app.handle().clone();
    let _ = app.listen("voice-state-changed", move |_event| {
        sync_tray_checks(&handle);
    });

    // Listen for self-talking events to update the tray icon overlay.
    let handle2 = app.handle().clone();
    let _ = app.listen("user-talking", move |event| {
        // Payload is [session, talking] (a JSON tuple).
        // We only care about our own session.
        if let Ok(payload) = serde_json::from_str::<(u32, bool)>(event.payload()) {
            let state = handle2.state::<AppState>();
            let own = state.get_own_session();
            if own == Some(payload.0) {
                update_tray_icon(&handle2, payload.1);
            }
        }
    });

    info!("System tray icon created");
    Ok(())
}

/// Update the tray icon to reflect the current talking state.
///
/// - `talking` = true  -> green-dot icon
/// - `talking` = false -> normal icon
fn update_tray_icon(app: &AppHandle, talking: bool) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let icon_bytes = if talking {
        render_talking_icon()
    } else {
        ICON_BYTES.to_vec()
    };

    match Image::from_bytes(&icon_bytes) {
        Ok(img) => {
            let _ = tray.set_icon(Some(img));
        }
        Err(e) => warn!("Failed to update tray icon: {e}"),
    }
}

/// Sync the Mute / Deafen check-menu items to the current `VoiceState`.
fn sync_tray_checks(app: &AppHandle) {
    let state = app.state::<AppState>();
    let vs = state.voice_state();
    let muted = matches!(vs, crate::state::VoiceState::Muted);
    let deafened = matches!(vs, crate::state::VoiceState::Inactive);

    if let Some(item) = MUTE_ITEM.get() {
        let _ = item.set_checked(muted);
    }
    if let Some(item) = DEAFEN_ITEM.get() {
        let _ = item.set_checked(deafened);
    }
}

/// Enable or disable the Mute / Deafen / Disconnect items based on
/// whether we are connected to a server.
fn set_connection_items_enabled(enabled: bool) {
    if let Some(item) = MUTE_ITEM.get() {
        let _ = item.set_enabled(enabled);
    }
    if let Some(item) = DEAFEN_ITEM.get() {
        let _ = item.set_enabled(enabled);
    }
    if let Some(item) = DISCONNECT_ITEM.get() {
        let _ = item.set_enabled(enabled);
    }
}

// -- Menu event handler ---------------------------------------------------

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        ID_MUTE => {
            let handle = app.clone();
            drop(tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                if let Err(e) = state.toggle_mute().await {
                    warn!("Tray: toggle mute failed: {e}");
                }
                sync_tray_checks(&handle);
            }));
        }
        ID_DEAFEN => {
            let handle = app.clone();
            drop(tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                if let Err(e) = state.toggle_deafen().await {
                    warn!("Tray: toggle deafen failed: {e}");
                }
                sync_tray_checks(&handle);
            }));
        }
        ID_DISCONNECT => {
            let handle = app.clone();
            drop(tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                if let Err(e) = state.disconnect().await {
                    warn!("Tray: disconnect failed: {e}");
                }
            }));
        }
        ID_SHOW => {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        ID_QUIT => {
            app.exit(0);
        }
        _ => {}
    }
}

// -- Talking icon rendering -----------------------------------------------

/// Render the app icon with a green dot overlay to indicate talking.
///
/// Decodes the embedded PNG, draws a green circle in the bottom-right
/// corner, and re-encodes as PNG. Falls back to the unmodified icon on error.
fn render_talking_icon() -> Vec<u8> {
    render_talking_icon_inner().unwrap_or_else(|| ICON_BYTES.to_vec())
}

/// Inner implementation that can fail gracefully.
fn render_talking_icon_inner() -> Option<Vec<u8>> {
    // Decode the base icon.
    let decoder = png::Decoder::new(std::io::Cursor::new(ICON_BYTES));
    let mut reader = decoder.read_info().ok()?;
    let buf_size = reader.output_buffer_size()?;
    let mut buf = vec![0u8; buf_size];
    let info = reader.next_frame(&mut buf).ok()?;
    let width = info.width as usize;
    let height = info.height as usize;
    buf.truncate(info.buffer_size());

    // Draw a filled green circle in the bottom-right quadrant.
    let radius = (width.min(height) as f64) * 0.15;
    let cx = width as f64 - radius - 2.0;
    let cy = height as f64 - radius - 2.0;

    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= radius {
                let i = (y * width + x) * 4;
                if i + 3 < buf.len() {
                    buf[i] = 76;      // R
                    buf[i + 1] = 175; // G
                    buf[i + 2] = 80;  // B
                    buf[i + 3] = 255; // A
                }
            } else if dist <= radius + 1.5 {
                // Anti-aliased edge: white ring for contrast.
                let i = (y * width + x) * 4;
                if i + 3 < buf.len() {
                    let alpha = ((radius + 1.5 - dist) / 1.5 * 255.0) as u8;
                    buf[i] = 255;
                    buf[i + 1] = 255;
                    buf[i + 2] = 255;
                    buf[i + 3] = alpha;
                }
            }
        }
    }

    // Re-encode as PNG.
    let mut out = Vec::new();
    {
        let mut encoder =
            png::Encoder::new(std::io::Cursor::new(&mut out), width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&buf).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn talking_icon_is_valid_png() {
        let data = render_talking_icon();
        // PNG magic bytes.
        assert_eq!(&data[..4], &[0x89, b'P', b'N', b'G']);
        assert!(data.len() > 100);
    }

    #[test]
    fn talking_icon_differs_from_base() {
        let talking = render_talking_icon();
        assert_ne!(talking.as_slice(), ICON_BYTES);
    }
}
