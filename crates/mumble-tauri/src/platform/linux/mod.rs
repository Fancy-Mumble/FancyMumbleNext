//! Linux-specific platform integrations.
//!
//! - [`desktop`]: `.desktop` file, icon installation, quick-action IPC.
//! - [`webview`]: `WebKitGTK` / `AppImage` environment workarounds.

pub(crate) mod desktop;
pub(crate) mod webview;
