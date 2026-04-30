//! Shared utility functions for the Fancy Mumble workspace.
//!
//! This crate provides small, dependency-free helpers that are useful
//! across multiple crates in the workspace.

pub mod audio;
pub mod fuzzy;
pub mod hex;
pub mod html;
pub mod image_filter;
pub mod net;
/// Mumble ACL permission bit definitions.
///
/// Single source of truth shared with the React frontend via the
/// `mumble-tauri` build script (regenerates `ui/src/utils/permissions.ts`).
/// Mirrors `enum ChanACL::Perm` in the Mumble server's `ACL.h`.
pub mod permissions;
pub mod version;
