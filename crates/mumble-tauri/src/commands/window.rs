//! Per-window constraints exposed to the frontend.
//!
//! These commands wrap the platform-specific helpers in
//! [`crate::platform::window`].  They operate on the calling window
//! (resolved automatically by Tauri via the `window` parameter), so
//! every webview window gets its own independent constraints.

use crate::platform::window::{WindowExt, WindowExtError};

/// Constrain (or release) the calling window's content aspect ratio.
///
/// Pass `Some(width / height)` to lock the ratio - native resize
/// gestures will be clamped without flicker.  Pass `None` (or omit
/// the field) to remove the constraint.
///
/// Returns:
/// - `Ok(true)`  - native constraint installed.
/// - `Ok(false)` - this platform has no native implementation; the
///   frontend should fall back to a JS resize handler.
/// - `Err(...)`  - the call reached the native layer but failed.
#[tauri::command]
pub(crate) fn set_window_aspect_ratio(
    window: tauri::WebviewWindow,
    ratio: Option<f64>,
) -> Result<bool, String> {
    match window.set_aspect_ratio(ratio) {
        Ok(()) => Ok(true),
        Err(WindowExtError::Unsupported) => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}
