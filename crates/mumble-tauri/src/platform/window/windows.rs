//! Windows aspect-ratio constraint via `WM_SIZING` window subclassing.
//!
//! Behaviour:
//! 1. [`WindowsAspectRatio::install`] registers a per-HWND ratio in a
//!    process-global registry and installs an idempotent
//!    `SetWindowSubclass` hook.
//! 2. The subclass proc handles `WM_SIZING`, snapping the proposed
//!    drag rect's client area to the configured ratio before the OS
//!    commits the new geometry - zero flicker, no JS feedback loop.
//! 3. `WM_NCDESTROY` cleans the registry entry automatically.
//! 4. [`WindowsAspectRatio::uninstall`] clears the entry and removes
//!    the subclass.
//!
//! All logic is grouped on [`WindowsAspectRatio`].  The C trampoline
//! must remain an `extern "system" fn` (Win32 ABI requirement) but
//! lives as an associated function on the same impl block.

#![allow(
    unsafe_code,
    reason = "Win32 window subclassing requires raw FFI; \
              every unsafe block has a SAFETY comment justifying it."
)]

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use tauri::WebviewWindow;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
    WMSZ_BOTTOM, WMSZ_BOTTOMLEFT, WMSZ_BOTTOMRIGHT, WMSZ_LEFT, WMSZ_RIGHT, WMSZ_TOP,
    WMSZ_TOPLEFT, WMSZ_TOPRIGHT, WM_NCDESTROY, WM_SIZING,
};

use super::{AspectRatioConstraint, WindowExtError};

/// Subclass id - chosen as the ASCII for "FANM" so it does not collide
/// with any system or third-party subclass we might share an HWND with.
const SUBCLASS_ID: usize = 0x46414E4D;

/// Process-global HWND -> ratio map.  Shared between
/// [`WindowsAspectRatio`] and the C subclass trampoline.
fn registry() -> &'static Mutex<HashMap<isize, f64>> {
    static REG: OnceLock<Mutex<HashMap<isize, f64>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_registry() -> MutexGuard<'static, HashMap<isize, f64>> {
    // Recover from poisoning: failing the resize hook is worse than
    // continuing with a (single) corrupted entry.
    registry().lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Windows backend for [`AspectRatioConstraint`].  Stateless - all
/// per-window data lives in [`registry`], keyed by HWND.
pub(super) struct WindowsAspectRatio;

impl AspectRatioConstraint for WindowsAspectRatio {
    fn install(&self, win: &WebviewWindow, ratio: f64) -> Result<(), WindowExtError> {
        let (hwnd, key) = Self::hwnd_pair(win)?;
        Self::install_hook(hwnd, key, ratio)?;
        Self::snap_to_ratio(hwnd, ratio);
        Ok(())
    }

    fn uninstall(&self, win: &WebviewWindow) -> Result<(), WindowExtError> {
        let (hwnd, key) = Self::hwnd_pair(win)?;
        Self::uninstall_hook(hwnd, key);
        Ok(())
    }
}

impl WindowsAspectRatio {
    /// Resolve the Tauri window's HWND into the raw pointer plus an
    /// `isize` registry key.
    fn hwnd_pair(win: &WebviewWindow) -> Result<(HWND, isize), WindowExtError> {
        let hwnd = win
            .hwnd()
            .map_err(|e| WindowExtError::NoHandle(e.to_string()))?;
        // tauri's HWND wraps `*mut c_void`; cast through isize so it
        // is both a `Hash`/`Eq` registry key and a windows-sys HWND.
        let key = hwnd.0 as isize;
        Ok((key as HWND, key))
    }

    /// Insert the entry and (if not already present) install the
    /// subclass.  Idempotent for the same HWND.
    fn install_hook(hwnd: HWND, key: isize, ratio: f64) -> Result<(), WindowExtError> {
        let was_present = {
            let mut map = lock_registry();
            let existed = map.contains_key(&key);
            let _previous = map.insert(key, ratio);
            existed
        };
        if was_present {
            return Ok(());
        }
        // SAFETY: SetWindowSubclass is documented to be safe on the
        // thread that owns the HWND.  Tauri commands run on the UI
        // thread so this holds.
        let ok = unsafe { SetWindowSubclass(hwnd, Some(Self::subclass_proc), SUBCLASS_ID, 0) };
        if ok == 0 {
            let _removed = lock_registry().remove(&key);
            return Err(WindowExtError::Native(
                "SetWindowSubclass returned FALSE".into(),
            ));
        }
        Ok(())
    }

    /// Remove the registry entry and detach the subclass.
    fn uninstall_hook(hwnd: HWND, key: isize) {
        let removed = lock_registry().remove(&key).is_some();
        if removed {
            // SAFETY: see `install_hook`.
            let _ok =
                unsafe { RemoveWindowSubclass(hwnd, Some(Self::subclass_proc), SUBCLASS_ID) };
        }
    }

    /// Resize the window now so its current geometry matches the
    /// configured ratio.  Without this the constraint would only kick
    /// in on the next user drag.
    fn snap_to_ratio(hwnd: HWND, ratio: f64) {
        // SAFETY: GetWindowRect / GetClientRect / SetWindowPos accept
        // any valid HWND and validate it internally.  Failure is
        // ignored so a destroyed HWND becomes a no-op.
        unsafe {
            let mut wr: RECT = std::mem::zeroed();
            let mut cr: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut wr) == 0 || GetClientRect(hwnd, &mut cr) == 0 {
                return;
            }
            let frame_w = (wr.right - wr.left) - (cr.right - cr.left);
            let frame_h = (wr.bottom - wr.top) - (cr.bottom - cr.top);
            let cw = (cr.right - cr.left).max(1);
            let target_ch = ((cw as f64 / ratio).round() as i32).max(1);
            let new_w = cw + frame_w;
            let new_h = target_ch + frame_h;
            let _ok = SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                new_w,
                new_h,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    /// Compute non-client frame thickness so we can convert between
    /// window coords (used by `WM_SIZING` rects) and client coords
    /// (used by the ratio).
    fn frame_size(hwnd: HWND) -> (i32, i32) {
        // SAFETY: HWND has just been used by the subclass proc, so it
        // is valid for the duration of this call.
        unsafe {
            let mut wr: RECT = std::mem::zeroed();
            let mut cr: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut wr) == 0 || GetClientRect(hwnd, &mut cr) == 0 {
                return (0, 0);
            }
            (
                (wr.right - wr.left) - (cr.right - cr.left),
                (wr.bottom - wr.top) - (cr.bottom - cr.top),
            )
        }
    }

    /// Adjust a `WM_SIZING` rect in-place so its client area matches
    /// the requested aspect ratio.  The driving edge (encoded in
    /// `wparam`) determines which dimension is fixed and which is
    /// computed.
    fn apply_to_sizing_rect(hwnd: HWND, edge: u32, rect: &mut RECT, ratio: f64) {
        let (frame_w, frame_h) = Self::frame_size(hwnd);

        let cw = (rect.right - rect.left - frame_w).max(1);
        let ch = (rect.bottom - rect.top - frame_h).max(1);

        let (new_cw, new_ch) = match edge {
            WMSZ_LEFT | WMSZ_RIGHT => (cw, ((cw as f64 / ratio).round() as i32).max(1)),
            WMSZ_TOP | WMSZ_BOTTOM => (((ch as f64 * ratio).round() as i32).max(1), ch),
            _ => {
                // Corner drag: pick the dimension that grew more so
                // the cursor stays close to the dragged corner.
                let from_w = (cw, ((cw as f64 / ratio).round() as i32).max(1));
                let from_h = (((ch as f64 * ratio).round() as i32).max(1), ch);
                if from_w.0 * from_w.1 >= from_h.0 * from_h.1 {
                    from_w
                } else {
                    from_h
                }
            }
        };

        let new_w = new_cw + frame_w;
        let new_h = new_ch + frame_h;

        // Anchor the rect on the opposite side of the dragged edge so
        // the non-driving edge stays put under the user's cursor.
        match edge {
            WMSZ_LEFT => {
                rect.left = rect.right - new_w;
                rect.bottom = rect.top + new_h;
            }
            WMSZ_RIGHT => {
                rect.right = rect.left + new_w;
                rect.bottom = rect.top + new_h;
            }
            WMSZ_TOP => {
                rect.top = rect.bottom - new_h;
                rect.right = rect.left + new_w;
            }
            WMSZ_BOTTOM => {
                rect.bottom = rect.top + new_h;
                rect.right = rect.left + new_w;
            }
            WMSZ_TOPLEFT => {
                rect.left = rect.right - new_w;
                rect.top = rect.bottom - new_h;
            }
            WMSZ_TOPRIGHT => {
                rect.right = rect.left + new_w;
                rect.top = rect.bottom - new_h;
            }
            WMSZ_BOTTOMLEFT => {
                rect.left = rect.right - new_w;
                rect.bottom = rect.top + new_h;
            }
            WMSZ_BOTTOMRIGHT => {
                rect.right = rect.left + new_w;
                rect.bottom = rect.top + new_h;
            }
            _ => {}
        }
    }

    /// C-ABI subclass trampoline.  Must be a free `extern "system" fn`
    /// (Win32 ABI requirement) - it lives as an associated function so
    /// every piece of the Windows backend stays grouped on this impl.
    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _ref_data: usize,
    ) -> LRESULT {
        if msg == WM_SIZING {
            if let Some(ratio) = lock_registry().get(&(hwnd as isize)).copied() {
                // SAFETY: per WM_SIZING contract, lparam is a valid
                // pointer to a RECT for the duration of the message.
                let rect = unsafe { &mut *(lparam as *mut RECT) };
                Self::apply_to_sizing_rect(hwnd, wparam as u32, rect, ratio);
                return 1; // TRUE - tell the OS to use our modified rect
            }
        }
        if msg == WM_NCDESTROY {
            let _removed = lock_registry().remove(&(hwnd as isize));
        }
        // SAFETY: DefSubclassProc forwards to the next subclass or
        // window proc; the HWND/msg/params are exactly what we received.
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}
