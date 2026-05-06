//! macOS aspect-ratio constraint via `NSWindow.contentAspectRatio`.
//!
//! AppKit has first-class support: setting `contentAspectRatio` to a
//! non-zero `NSSize` makes the OS clamp every live drag-resize to that
//! ratio.  We translate the requested `width / height` ratio into an
//! `NSSize` (using the natural-image dimensions would be ideal, but
//! any pair with the right ratio works because AppKit only looks at
//! the proportion).
//!
//! Setting `NSSize { width: 0.0, height: 0.0 }` clears the constraint.
//! After installing we additionally call `setContentSize:` to snap the
//! current window to the ratio so the user sees the change immediately
//! (AppKit only enforces the ratio on subsequent drags otherwise).

#![allow(
    unsafe_code,
    reason = "AppKit interop requires `objc2` message sends; \
              every unsafe block has a SAFETY comment justifying it."
)]

use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSRect, NSSize};
use tauri::WebviewWindow;

use super::{AspectRatioConstraint, WindowExtError};

/// macOS backend for [`AspectRatioConstraint`].  Stateless - all state
/// lives on the `NSWindow` itself via AppKit's built-in property.
pub(super) struct MacosAspectRatio;

impl AspectRatioConstraint for MacosAspectRatio {
    fn install(&self, win: &WebviewWindow, ratio: f64) -> Result<(), WindowExtError> {
        let ns_window = Self::ns_window(win)?;
        // Pick a width of 1000 so the height has plenty of resolution
        // for the chosen ratio without overflowing CGFloat precision.
        let size = NSSize {
            width: 1000.0,
            height: 1000.0 / ratio,
        };
        Self::set_aspect_ratio(ns_window, size);
        Self::snap_to_ratio(ns_window, ratio);
        Ok(())
    }

    fn uninstall(&self, win: &WebviewWindow) -> Result<(), WindowExtError> {
        let ns_window = Self::ns_window(win)?;
        // NSSize::ZERO removes the constraint per Apple docs.
        Self::set_aspect_ratio(ns_window, NSSize { width: 0.0, height: 0.0 });
        Ok(())
    }
}

impl MacosAspectRatio {
    fn ns_window(win: &WebviewWindow) -> Result<*mut AnyObject, WindowExtError> {
        let ptr = win
            .ns_window()
            .map_err(|e| WindowExtError::NoHandle(e.to_string()))?;
        Ok(ptr.cast::<AnyObject>())
    }

    fn set_aspect_ratio(ns_window: *mut AnyObject, size: NSSize) {
        // SAFETY: `ns_window` was obtained from Tauri's
        // `WebviewWindow::ns_window`, which guarantees a live
        // `NSWindow*` for the duration of this call.  Method must be
        // invoked on the main thread; Tauri commands already are.
        unsafe {
            let _: () = msg_send![ns_window, setContentAspectRatio: size];
        }
    }

    /// Resize the window now so its current geometry matches the
    /// configured ratio.
    fn snap_to_ratio(ns_window: *mut AnyObject, ratio: f64) {
        // SAFETY: see `set_aspect_ratio`.
        unsafe {
            // `frame` returns the window rect (incl. title bar) but we
            // only need its width to derive a matching height; AppKit
            // converts the resulting `setContentSize:` call back to
            // content coords for us.
            let frame: NSRect = msg_send![ns_window, frame];
            let cw = frame.size.width.max(1.0);
            let new_size = NSSize {
                width: cw,
                height: cw / ratio,
            };
            let _: () = msg_send![ns_window, setContentSize: new_size];
        }
    }
}

/// Toggle window-server capture exclusion via `NSWindow.sharingType`.
///
/// Setting `NSWindowSharingNone` (= 0) hides the window from
/// `CGWindowListCreateImage`, screen-recording APIs and most
/// third-party capture stacks. Reverting to `NSWindowSharingReadOnly`
/// (= 1, the default) makes the window capturable again.
pub(super) fn set_excluded_from_capture(
    win: &WebviewWindow,
    excluded: bool,
) -> Result<(), WindowExtError> {
    let ns_window = win
        .ns_window()
        .map_err(|e| WindowExtError::NoHandle(e.to_string()))?
        .cast::<AnyObject>();
    let sharing_type: usize = if excluded { 0 } else { 1 };
    // SAFETY: see `MacosAspectRatio::set_aspect_ratio`.
    unsafe {
        let _: () = msg_send![ns_window, setSharingType: sharing_type];
    }
    Ok(())
}
