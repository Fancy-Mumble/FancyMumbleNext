//! Windows-only helpers that locate the top-level window currently
//! being shared (matched by client-area dimensions) and follow it as
//! the user drags / resizes it, so the desktop drawing overlay stays
//! pinned over the actual shared content.
//!
//! The tracker runs as a Tokio task at a low polling rate (100 ms) -
//! cheaper than installing `WinEvent` hooks, and quite responsive on
//! every modern Windows version.

#![allow(
    unsafe_code,
    reason = "All Win32 calls are wrapped with explicit SAFETY notes; \
              the FFI surface is well-defined and validated by the OS."
)]

use std::ffi::c_void;
use std::time::Duration;

use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager};
use tokio::time::sleep;
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetForegroundWindow, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible,
};

use super::DRAW_OVERLAY_LABEL;

/// Pixel rect of a window's client area in screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Find a top-level visible window whose client area matches the
/// given pixel size, preferring the foreground window when several
/// candidates qualify.
pub(super) fn find_window_by_client_size(width: u32, height: u32) -> Option<isize> {
    if width == 0 || height == 0 {
        return None;
    }
    let mut ctx = SearchCtx {
        target_w: width as i32,
        target_h: height as i32,
        own_pid: unsafe { GetCurrentProcessId() },
        candidates: Vec::new(),
    };
    let lparam = (&raw mut ctx) as *mut c_void as isize;
    // SAFETY: `enum_proc` is a valid `extern "system" fn`; lparam carries
    // a pointer to `ctx` which outlives the EnumWindows call.
    let _ = unsafe { EnumWindows(Some(enum_proc), lparam) };
    if ctx.candidates.is_empty() {
        return None;
    }
    // Prefer the current foreground window; otherwise the first hit
    // (EnumWindows returns top-most z-order first on modern Windows).
    let fg = unsafe { GetForegroundWindow() } as isize;
    if ctx.candidates.contains(&fg) {
        return Some(fg);
    }
    Some(ctx.candidates[0])
}

/// Read the screen rect of `hwnd`'s client area, or `None` if the
/// window has been destroyed or has zero size.
pub(super) fn screen_rect_of(hwnd: isize) -> Option<ScreenRect> {
    let h = hwnd as HWND;
    // SAFETY: IsWindow accepts any HWND (even invalid) and returns 0 in that case.
    if unsafe { IsWindow(h) } == 0 {
        return None;
    }
    let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: client is a writable RECT; the OS fills it on success.
    if unsafe { GetClientRect(h, &raw mut client) } == 0 {
        return None;
    }
    let w = client.right - client.left;
    let bh = client.bottom - client.top;
    if w <= 0 || bh <= 0 {
        return None;
    }
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY: ClientToScreen translates the (0,0) client point into screen coords.
    if unsafe { ClientToScreen(h, &raw mut origin) } == 0 {
        return None;
    }
    Some(ScreenRect { x: origin.x, y: origin.y, w, h: bh })
}

/// Spawn a background task that polls `hwnd`'s screen rect and keeps
/// the overlay window pinned over it.  The task exits (and closes the
/// overlay) when the source window disappears or the overlay window is
/// closed externally.
pub(super) fn spawn_tracker(app: AppHandle, hwnd: isize) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last: Option<ScreenRect> = None;
        loop {
            sleep(Duration::from_millis(100)).await;
            let Some(window) = app.get_webview_window(DRAW_OVERLAY_LABEL) else {
                return;
            };
            let Some(rect) = screen_rect_of(hwnd) else {
                let _ = window.close();
                return;
            };
            if Some(rect) == last {
                continue;
            }
            last = Some(rect);
            // Convert physical pixels to logical units for Tauri.
            let scale = window.scale_factor().unwrap_or(1.0);
            let _ = window.set_position(LogicalPosition::new(
                f64::from(rect.x) / scale,
                f64::from(rect.y) / scale,
            ));
            let _ = window.set_size(LogicalSize::new(
                f64::from(rect.w) / scale,
                f64::from(rect.h) / scale,
            ));
        }
    })
}

// ---------------------------------------------------------------------------
// EnumWindows callback plumbing
// ---------------------------------------------------------------------------

struct SearchCtx {
    target_w: i32,
    target_h: i32,
    own_pid: u32,
    candidates: Vec<isize>,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam was set to `&mut SearchCtx as *mut c_void as isize`
    // in `find_window_by_client_size`; the pointer is valid for the
    // duration of EnumWindows.
    let ctx = &mut *(lparam as *mut c_void as *mut SearchCtx);

    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1;
    }
    // Skip windows owned by our own process (the broadcaster app
    // itself) so we don't latch onto our own preview window.
    let mut pid: u32 = 0;
    let _ = unsafe { GetWindowThreadProcessId(hwnd, &raw mut pid) };
    if pid == ctx.own_pid {
        return 1;
    }
    let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    if unsafe { GetClientRect(hwnd, &raw mut client) } == 0 {
        return 1;
    }
    let w = client.right - client.left;
    let h = client.bottom - client.top;
    if w == ctx.target_w && h == ctx.target_h {
        ctx.candidates.push(hwnd as isize);
    }
    1 // continue enumeration
}
