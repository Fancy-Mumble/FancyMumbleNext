//! Drawing overlay window: a transparent, click-through, always-on-top
//! window pinned over the broadcaster's shared content so they can see
//! viewer drawings on their actual screen / window.
//!
//! Properties (set at window creation):
//! - **Transparent** background so only the strokes paint over the
//!   real desktop / window.
//! - **Always-on-top** so the overlay sits above the captured source.
//! - **Click-through** (`set_ignore_cursor_events(true)`) - all
//!   pointer events pass through to whatever is underneath.
//! - **Excluded from screen capture** (`WDA_EXCLUDEFROMCAPTURE` on
//!   Windows, `NSWindowSharingNone` on macOS) so the strokes do not
//!   appear in the broadcaster's outgoing stream.
//!
//! Sizing strategy (in priority order):
//! 1. `display_surface == "window"` on Windows: enumerate top-level
//!    windows for one whose client area matches the captured size and
//!    pin the overlay over its screen rect, then poll for movement.
//! 2. `display_surface == "monitor"`: pick the monitor whose pixel
//!    dimensions match the captured size and cover it fully.
//! 3. Fallback: monitor under the cursor, then primary monitor.
//!
//! At most one overlay window per app process; reopening replaces the
//! previous one.

use crate::state::AppState;

#[cfg(not(target_os = "android"))]
use crate::platform::window::WindowExt;

#[cfg(target_os = "windows")]
mod win_tracker;

/// Stable label used for the (single) drawing-overlay window.
/// Picked up by the frontend's `App.tsx` window-kind dispatcher.
pub(crate) const DRAW_OVERLAY_LABEL: &str = "draw-overlay";

/// Payload picked up by the freshly-opened drawing-overlay window.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DrawOverlayContext {
    /// Channel the overlay should mirror strokes for.
    pub channel_id: u32,
    /// The local user's session id (kept for symmetry with the in-app
    /// `DrawingOverlay`; the overlay only renders, never sends).
    pub own_session: u32,
}

#[cfg(not(target_os = "android"))]
#[derive(Copy, Clone)]
struct OverlayPlacement {
    /// Logical-units position (top-left).
    x: f64,
    y: f64,
    /// Logical-units size.
    w: f64,
    h: f64,
    /// Optional Windows HWND of the source window to follow.
    #[cfg(target_os = "windows")]
    hwnd_to_track: Option<isize>,
}

/// Open the drawing-overlay window for `channel_id`.  Replaces any
/// existing overlay.  No-op on Android.
///
/// `capture_width` / `capture_height` are the pixel dimensions of the
/// shared video track (from `MediaStreamTrack.getSettings()`).
/// `display_surface` is `"monitor"`, `"window"`, `"browser"` or
/// `"application"` (also from `getSettings()`); when `"window"` we try
/// to find the matching top-level window and pin the overlay over it.
#[cfg(not(target_os = "android"))]
#[tauri::command]
pub(crate) async fn open_drawing_overlay(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    own_session: u32,
    capture_width: Option<u32>,
    capture_height: Option<u32>,
    display_surface: Option<String>,
) -> Result<(), String> {
    use tauri::Manager;

    if let Ok(mut slot) = state.draw_overlay_context.lock() {
        *slot = Some(DrawOverlayContext { channel_id, own_session });
    }

    // Tear down any previous overlay + tracker before opening a new one.
    abort_tracker(&state);
    if let Some(existing) = app.get_webview_window(DRAW_OVERLAY_LABEL) {
        let _ = existing.close();
    }

    let placement = compute_placement(&app, capture_width, capture_height, display_surface.as_deref())?;

    let window = tauri::WebviewWindowBuilder::new(
        &app,
        DRAW_OVERLAY_LABEL,
        tauri::WebviewUrl::App(std::path::PathBuf::from("index.html")),
    )
    .title("")
    .decorations(false)
    .shadow(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .inner_size(placement.w, placement.h)
    .position(placement.x, placement.y)
    .build()
    .map_err(|e: tauri::Error| e.to_string())?;

    if let Err(e) = window.set_ignore_cursor_events(true) {
        tracing::warn!("draw-overlay: set_ignore_cursor_events failed: {e}");
    }
    if let Err(e) = window.set_excluded_from_capture(true) {
        tracing::warn!("draw-overlay: capture exclusion not applied: {e}");
    }

    spawn_tracker_if_supported(&app, &state, placement);

    Ok(())
}

/// Decide where to put the overlay window.
#[cfg(not(target_os = "android"))]
fn compute_placement(
    app: &tauri::AppHandle,
    capture_width: Option<u32>,
    capture_height: Option<u32>,
    display_surface: Option<&str>,
) -> Result<OverlayPlacement, String> {
    if matches!(display_surface, Some("window") | Some("application")) {
        if let Some(p) = window_share_placement(app, capture_width, capture_height) {
            return Ok(p);
        }
        tracing::info!(
            "draw-overlay: window-share requested but no matching top-level window \
             found ({capture_width:?}x{capture_height:?}); falling back to monitor"
        );
    }
    monitor_placement(app, capture_width, capture_height)
}

/// Try to find a top-level window with matching client size.  Windows-only.
#[cfg(target_os = "windows")]
fn window_share_placement(
    app: &tauri::AppHandle,
    capture_width: Option<u32>,
    capture_height: Option<u32>,
) -> Option<OverlayPlacement> {
    let (w, h) = (capture_width?, capture_height?);
    let hwnd = win_tracker::find_window_by_client_size(w, h)?;
    let rect = win_tracker::screen_rect_of(hwnd)?;
    // Use the primary monitor's scale factor as a stand-in; Tauri
    // reapplies the right scale when the window appears on its target
    // monitor.  The tracker re-aligns immediately afterwards anyway.
    let scale = primary_scale(app);
    Some(OverlayPlacement {
        x: f64::from(rect.x) / scale,
        y: f64::from(rect.y) / scale,
        w: f64::from(rect.w) / scale,
        h: f64::from(rect.h) / scale,
        hwnd_to_track: Some(hwnd),
    })
}

#[cfg(all(not(target_os = "android"), not(target_os = "windows")))]
fn window_share_placement(
    _app: &tauri::AppHandle,
    _capture_width: Option<u32>,
    _capture_height: Option<u32>,
) -> Option<OverlayPlacement> {
    None
}

#[cfg(target_os = "windows")]
fn primary_scale(app: &tauri::AppHandle) -> f64 {
    app.primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0)
}

/// Choose the monitor that the overlay should cover and turn it into
/// an [`OverlayPlacement`].
#[cfg(not(target_os = "android"))]
fn monitor_placement(
    app: &tauri::AppHandle,
    capture_width: Option<u32>,
    capture_height: Option<u32>,
) -> Result<OverlayPlacement, String> {
    let monitor = pick_target_monitor(app, capture_width, capture_height)?;
    let size = monitor.size();
    let position = monitor.position();
    let scale = monitor.scale_factor();
    Ok(OverlayPlacement {
        x: f64::from(position.x) / scale,
        y: f64::from(position.y) / scale,
        w: f64::from(size.width) / scale,
        h: f64::from(size.height) / scale,
        #[cfg(target_os = "windows")]
        hwnd_to_track: None,
    })
}

/// Choose the monitor that the overlay should cover.
///
/// Priority:
/// 1. A monitor whose pixel size matches `capture_width` x `capture_height`.
/// 2. The monitor under the cursor right now.
/// 3. The primary monitor.
#[cfg(not(target_os = "android"))]
fn pick_target_monitor(
    app: &tauri::AppHandle,
    capture_width: Option<u32>,
    capture_height: Option<u32>,
) -> Result<tauri::Monitor, String> {
    let monitors = app
        .available_monitors()
        .map_err(|e| format!("available_monitors failed: {e}"))?;

    if let (Some(w), Some(h)) = (capture_width, capture_height) {
        if let Some(m) = monitors
            .iter()
            .find(|m| m.size().width == w && m.size().height == h)
        {
            return Ok(m.clone());
        }
    }

    if let Ok(pos) = app.cursor_position() {
        if let Some(m) = monitors.iter().find(|m| {
            let mp = m.position();
            let ms = m.size();
            let x = pos.x as i32;
            let y = pos.y as i32;
            x >= mp.x
                && y >= mp.y
                && x < mp.x + ms.width as i32
                && y < mp.y + ms.height as i32
        }) {
            return Ok(m.clone());
        }
    }

    app.primary_monitor()
        .map_err(|e| format!("primary_monitor failed: {e}"))?
        .ok_or_else(|| "no primary monitor available".to_string())
}

#[cfg(target_os = "windows")]
fn spawn_tracker_if_supported(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, AppState>,
    placement: OverlayPlacement,
) {
    let Some(hwnd) = placement.hwnd_to_track else { return; };
    let handle = win_tracker::spawn_tracker(app.clone(), hwnd);
    if let Ok(mut slot) = state.draw_overlay_tracker.lock() {
        *slot = Some(handle);
    }
}

#[cfg(all(not(target_os = "android"), not(target_os = "windows")))]
fn spawn_tracker_if_supported(
    _app: &tauri::AppHandle,
    _state: &tauri::State<'_, AppState>,
    _placement: OverlayPlacement,
) {
}

#[cfg(not(target_os = "android"))]
fn abort_tracker(state: &tauri::State<'_, AppState>) {
    if let Ok(mut slot) = state.draw_overlay_tracker.lock() {
        if let Some(handle) = slot.take() {
            handle.abort();
        }
    }
}

#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) async fn open_drawing_overlay(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
    _channel_id: u32,
    _own_session: u32,
    _capture_width: Option<u32>,
    _capture_height: Option<u32>,
    _display_surface: Option<String>,
) -> Result<(), String> {
    Err("Drawing overlay windows are not supported on Android".to_string())
}

/// Close the currently-open drawing-overlay window, if any.
#[cfg(not(target_os = "android"))]
#[tauri::command]
pub(crate) async fn close_drawing_overlay(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    use tauri::Manager;

    abort_tracker(&state);
    if let Ok(mut slot) = state.draw_overlay_context.lock() {
        *slot = None;
    }
    if let Some(existing) = app.get_webview_window(DRAW_OVERLAY_LABEL) {
        let _ = existing.close();
    }
    Ok(())
}

#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) async fn close_drawing_overlay(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    Ok(())
}

/// Hand the overlay window the channel/session it should mirror.
/// Idempotent: returns the same context on repeated calls so a
/// reload (e.g. devtools refresh) still gets the data.
#[tauri::command]
pub(crate) fn take_drawing_overlay_context(
    state: tauri::State<'_, AppState>,
) -> Option<DrawOverlayContext> {
    state.draw_overlay_context.lock().ok().and_then(|m| m.clone())
}
