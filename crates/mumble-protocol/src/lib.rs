//! `mumble-protocol` - async Mumble client library.
//!
//! Provides a fully asynchronous Mumble client with:
//! - Simultaneous TCP (control) and UDP (audio) transports
//! - Priority work queue that favours low-latency audio
//! - Command-pattern API: each action is a self-contained struct
//! - Event handler trait for reacting to server messages
//!
//! # Architecture overview
//!
//! ```text
//!  [TCP reader] --+    [UDP reader] --+
//!                  v                  v
//!          [Priority Work Queue]
//!          (UDP > TCP > User commands)
//!                  |
//!          [Client Event Loop]
//!           - updates ServerState
//!           - invokes EventHandler
//!           - executes CommandAction outputs
//! ```

/// The Fancy Mumble extension version advertised by this crate,
/// derived from `Cargo.toml` at compile time.
pub const FANCY_VERSION: u64 = fancy_utils::version::fancy_version_encode(
    parse_u16(env!("CARGO_PKG_VERSION_MAJOR")),
    parse_u16(env!("CARGO_PKG_VERSION_MINOR")),
    parse_u16(env!("CARGO_PKG_VERSION_PATCH")),
);

/// Parse a `&str` to `u16` at compile time.
const fn parse_u16(s: &str) -> u16 {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut n: u16 = 0;
    while i < bytes.len() {
        n = n * 10 + (bytes[i] - b'0') as u16;
        i += 1;
    }
    n
}

pub mod audio;
pub mod client;
pub mod command;
pub mod error;
pub mod event;
pub mod message;
#[cfg(feature = "persistent-chat")]
pub mod persistent;
pub mod proto;
pub mod state;
pub mod transport;
pub mod work_queue;

// Suppress `unused_crate_dependencies` for crates that are only used in
// the integration / audio-quality test binaries under tests/, not in lib.rs.
#[cfg(test)]
mod _dev_deps {
    use hound as _;
    use minimp3 as _;
    use rcgen as _;
}
