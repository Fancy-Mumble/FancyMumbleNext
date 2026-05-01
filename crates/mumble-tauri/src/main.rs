//! Fancy Mumble desktop application entry point.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Dependencies are consumed by the library target; the binary entry point
// delegates entirely to `lib::run()` and has no direct imports.
#![allow(unused_crate_dependencies, reason = "all dependencies are used by the library target, not main.rs")]

fn main() {
    mumble_tauri_lib::platform::pre_init();

    mumble_tauri_lib::run();
}
