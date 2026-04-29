//! Linux/GTK aspect-ratio constraint via
//! `gtk_window_set_geometry_hints`.
//!
//! GTK exposes `min_aspect` / `max_aspect` window-manager hints; setting
//! both to the same value asks the WM to clamp every resize gesture to
//! that ratio.  Compliance varies by WM (mutter, kwin and most tiling
//! WMs honour it; some minimalist WMs ignore it).  When the WM does
//! enforce it, the snap is flicker-free because it happens before the
//! window is repainted.

use gtk::gdk::{Geometry, WindowHints};
use gtk::prelude::*;
use tauri::WebviewWindow;

use super::{AspectRatioConstraint, WindowExtError};

/// Linux backend for [`AspectRatioConstraint`].  Stateless - all state
/// lives on the `GtkWindow` itself via the geometry-hints property.
pub(super) struct LinuxAspectRatio;

impl AspectRatioConstraint for LinuxAspectRatio {
    fn install(&self, win: &WebviewWindow, ratio: f64) -> Result<(), WindowExtError> {
        let gtk_win = Self::gtk_window(win)?;
        Self::set_aspect_hint(&gtk_win, Some(ratio));
        Self::snap_to_ratio(&gtk_win, ratio);
        Ok(())
    }

    fn uninstall(&self, win: &WebviewWindow) -> Result<(), WindowExtError> {
        let gtk_win = Self::gtk_window(win)?;
        Self::set_aspect_hint(&gtk_win, None);
        Ok(())
    }
}

impl LinuxAspectRatio {
    fn gtk_window(win: &WebviewWindow) -> Result<gtk::ApplicationWindow, WindowExtError> {
        win.gtk_window()
            .map_err(|e| WindowExtError::NoHandle(e.to_string()))
    }

    fn set_aspect_hint(gtk_win: &gtk::ApplicationWindow, ratio: Option<f64>) {
        let mut geom = Geometry::new(0, 0, -1, -1, 0, 0, 0, 0, 0.0, 0.0, gtk::gdk::Gravity::NorthWest);
        let hints = match ratio {
            Some(r) => {
                geom.set_min_aspect(r);
                geom.set_max_aspect(r);
                WindowHints::ASPECT
            }
            None => WindowHints::empty(),
        };
        gtk_win.set_geometry_hints(gtk::Widget::NONE, Some(&geom), hints);
    }

    /// Resize the window now so its current geometry matches the
    /// configured ratio.
    fn snap_to_ratio(gtk_win: &gtk::ApplicationWindow, ratio: f64) {
        let (w, h) = gtk_win.size();
        let new_w = w.max(1);
        let new_h = ((f64::from(new_w) / ratio).round() as i32).max(1);
        if new_h != h {
            gtk_win.resize(new_w, new_h);
        }
    }
}
