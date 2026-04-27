//! Compile-time platform selection via the [`PlatformHooks`] trait.
//!
//! Platform-specific code is organised into sub-folders:
//!
//! - [`linux`]    - `.desktop` integration, `WebKitGTK` / `AppImage` workarounds.
//! - [`android`]  - foreground service bridge, FCM token retrieval.
//! - [`desktop`]  - system tray icon (all desktop OSes).
//! - [`badge`]    - taskbar badge overlay and system clock detection.

pub(crate) mod badge;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(not(target_os = "android"))]
pub(crate) mod desktop;
pub(crate) mod window;

/// Lifecycle hooks invoked at fixed points in the application startup sequence.
///
/// Default implementations are no-ops so adding a new hook never breaks
/// platforms that do not need it.
pub trait PlatformHooks {
    /// Called as the very first line of `main()` before shared libraries load.
    fn pre_init() {}

    /// Checks for required system dependencies; logs warnings for any absent.
    fn check_dependencies() {}

    /// Sets up process-wide environment variables before GTK / Tauri starts.
    fn init() {}

    /// Returns `true` when a running instance handled the invocation and
    /// this process should exit immediately.
    fn try_single_instance() -> bool {
        false
    }

    /// Called inside Tauri's `setup` callback once the `AppHandle` is live.
    fn setup(_handle: tauri::AppHandle) {}

    /// Called on `tauri::RunEvent::Exit`.
    fn teardown() {}
}

// ----- Linux -----------------------------------------------------------------

/// Linux platform implementation.
#[cfg(target_os = "linux")]
#[derive(Debug)]
pub struct LinuxPlatform;

#[cfg(target_os = "linux")]
impl PlatformHooks for LinuxPlatform {
    fn pre_init() {
        linux::webview::pre_init();
    }

    fn check_dependencies() {
        linux::webview::check_dependencies();
    }

    fn init() {
        linux::webview::init_platform();
    }

    fn try_single_instance() -> bool {
        linux::desktop::try_send_quick_action()
    }

    fn setup(handle: tauri::AppHandle) {
        linux::desktop::install_desktop_entry();
        linux::desktop::start_action_listener(handle);
    }

    fn teardown() {
        linux::desktop::cleanup_socket();
    }
}

// ----- No-op default (Windows, macOS, Android, …) ---------------------------

#[cfg(not(target_os = "linux"))]
#[derive(Debug)]
/// No-op platform hooks for non-Linux targets.
pub struct NoPlatform;

#[cfg(not(target_os = "linux"))]
impl PlatformHooks for NoPlatform {}

// ----- Active platform -------------------------------------------------------

#[cfg(target_os = "linux")]
type Active = LinuxPlatform;

#[cfg(not(target_os = "linux"))]
type Active = NoPlatform;

// ----- Free-function wrappers ------------------------------------------------

/// Calls [`PlatformHooks::pre_init`] for the active platform.
pub fn pre_init() {
    <Active as PlatformHooks>::pre_init();
}

/// Calls [`PlatformHooks::check_dependencies`] for the active platform.
pub fn check_dependencies() {
    <Active as PlatformHooks>::check_dependencies();
}

/// Calls [`PlatformHooks::init`] for the active platform.
pub fn init() {
    <Active as PlatformHooks>::init();
}

/// Calls [`PlatformHooks::try_single_instance`] for the active platform.
pub fn try_single_instance() -> bool {
    <Active as PlatformHooks>::try_single_instance()
}

/// Calls [`PlatformHooks::setup`] for the active platform.
pub fn setup(handle: tauri::AppHandle) {
    <Active as PlatformHooks>::setup(handle);
}

/// Calls [`PlatformHooks::teardown`] for the active platform.
pub fn teardown() {
    <Active as PlatformHooks>::teardown();
}
