//! Per-window constraints not exposed by Tauri's cross-platform API.
//!
//! Tauri 2 exposes `set_size`, `set_min_size`, `set_max_size`, but not
//! aspect-ratio constraints.  Implementing one in JS via the `resize`
//! event flickers because the OS already painted the new (wrong) size
//! before our handler runs.  The only flicker-free approach is to hook
//! the native resize loop:
//!
//! - Windows: subclass the `HWND` and intercept `WM_SIZING` to clamp
//!   the proposed rect before the OS commits it.
//! - macOS (TODO): call `[NSWindow setContentAspectRatio:]`.
//! - Linux/GTK (TODO): call `gtk_window_set_geometry_hints` with
//!   `min_aspect == max_aspect`.
//!
//! # Architecture
//!
//! Decoupled in two layers:
//!
//! 1. [`AspectRatioConstraint`] - trait every backend implements.
//!    Operates on a [`tauri::WebviewWindow`] and knows how to install
//!    / uninstall a `width / height` ratio constraint.
//! 2. [`platform_constraints`] - zero-sized factory returning the
//!    backend selected at compile time for the current `target_os`.
//!
//! [`WindowExt`] is a small extension trait on [`tauri::WebviewWindow`]
//! that forwards to the active backend, so call sites read like
//! `window.set_aspect_ratio(Some(16.0/9.0))?`.
//!
//! Targets without a real implementation use [`unsupported::Noop`]
//! which returns [`WindowExtError::Unsupported`] - callers can then
//! fall back to a post-hoc JS resize handler.

use std::error::Error;
use std::fmt;

use tauri::WebviewWindow;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(all(unix, not(any(target_os = "android", target_os = "macos", target_os = "ios"))))]
mod linux;

#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    all(unix, not(any(target_os = "android", target_os = "macos", target_os = "ios"))),
)))]
mod unsupported;

/// Errors raised by [`AspectRatioConstraint`] / [`WindowExt`] methods.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "variants are produced conditionally per target_os; \
              `Unsupported` is unused on Windows but real on macOS/Linux/Android."
)]
pub enum WindowExtError {
    /// The native handle for this window was not available
    /// (e.g. the window has already been closed).
    NoHandle(String),
    /// This platform does not implement the requested constraint.
    Unsupported,
    /// A native (OS) call failed.
    Native(String),
}

impl fmt::Display for WindowExtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHandle(msg) => write!(f, "native window handle unavailable: {msg}"),
            Self::Unsupported => f.write_str("not supported on this platform"),
            Self::Native(msg) => write!(f, "native call failed: {msg}"),
        }
    }
}

impl Error for WindowExtError {}

/// Native-resize-loop aspect ratio constraint.
///
/// Implementors hook the OS resize machinery so that user drag-resize
/// is clamped to a fixed `width / height` ratio without intermediate
/// repaints.  Implementations are expected to be idempotent and
/// thread-safe for invocation from the Tauri main thread.
pub trait AspectRatioConstraint {
    /// Install (or replace) the constraint on `win`.
    ///
    /// `ratio` is `width / height` and must be finite and positive.
    fn install(&self, win: &WebviewWindow, ratio: f64) -> Result<(), WindowExtError>;

    /// Remove any previously installed constraint from `win`.
    /// Calling on a window without a constraint is a no-op.
    fn uninstall(&self, win: &WebviewWindow) -> Result<(), WindowExtError>;
}

/// Returns the [`AspectRatioConstraint`] backend for the current
/// platform, picked at compile time.  The returned value is zero-sized
/// and cheap to construct on every call.
#[must_use]
pub fn platform_constraints() -> impl AspectRatioConstraint {
    #[cfg(target_os = "windows")]
    {
        windows::WindowsAspectRatio
    }
    #[cfg(target_os = "macos")]
    {
        macos::MacosAspectRatio
    }
    #[cfg(all(unix, not(any(target_os = "android", target_os = "macos", target_os = "ios"))))]
    {
        linux::LinuxAspectRatio
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        all(unix, not(any(target_os = "android", target_os = "macos", target_os = "ios"))),
    )))]
    {
        unsupported::Noop
    }
}

/// Extension trait for [`tauri::WebviewWindow`] that adds platform-native
/// constraints which Tauri itself does not expose.
pub trait WindowExt {
    /// Constrain the window's content aspect ratio (`width / height`).
    ///
    /// `Some(r)` installs the constraint; subsequent native resize
    /// gestures are clamped to that ratio with no visible flicker.
    /// `None` removes the constraint.
    ///
    /// Returns [`WindowExtError::Unsupported`] on platforms without a
    /// native implementation; the caller can then fall back to a
    /// post-hoc JS resize handler.
    fn set_aspect_ratio(&self, ratio: Option<f64>) -> Result<(), WindowExtError>;
}

impl WindowExt for WebviewWindow {
    fn set_aspect_ratio(&self, ratio: Option<f64>) -> Result<(), WindowExtError> {
        let backend = platform_constraints();
        match ratio {
            Some(r) if r.is_finite() && r > 0.0 => backend.install(self, r),
            _ => backend.uninstall(self),
        }
    }
}
