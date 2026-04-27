//! No-op fallback for platforms without a native aspect-ratio API
//! (currently macOS, Linux, Android).
//!
//! Returning [`WindowExtError::Unsupported`] (rather than `Ok(())`)
//! lets the frontend detect the gap and apply a JS resize handler as
//! a fallback.

use tauri::WebviewWindow;

use super::{AspectRatioConstraint, WindowExtError};

/// Backend used on every platform without a native implementation.
/// Both [`AspectRatioConstraint`] methods report
/// [`WindowExtError::Unsupported`].
pub(super) struct Noop;

impl AspectRatioConstraint for Noop {
    fn install(&self, _win: &WebviewWindow, _ratio: f64) -> Result<(), WindowExtError> {
        Err(WindowExtError::Unsupported)
    }

    fn uninstall(&self, _win: &WebviewWindow) -> Result<(), WindowExtError> {
        Err(WindowExtError::Unsupported)
    }
}
