//! Tauri command handlers, grouped into logical submodules.
//!
//! All `#[tauri::command]` functions live here, organised by feature
//! area.  The crate-root `lib.rs` only contains application bootstrap
//! and the `tauri::generate_handler!` registration.

pub(crate) mod admin;
pub(crate) mod audio;
pub(crate) mod certificates;
pub(crate) mod channels;
pub(crate) mod connection;
pub(crate) mod dm;
pub(crate) mod draw_overlay;
pub(crate) mod files;
pub(crate) mod image;
pub(crate) mod keyshare;
pub(crate) mod messaging;
pub(crate) mod offload;
pub(crate) mod popout;
pub(crate) mod profile;
pub(crate) mod public_servers;
pub(crate) mod realtime;
pub(crate) mod server;
pub(crate) mod servers;
pub(crate) mod system;
pub(crate) mod window;
