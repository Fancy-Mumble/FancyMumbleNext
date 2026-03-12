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

pub mod audio;
pub mod client;
pub mod command;
pub mod error;
pub mod event;
pub mod message;
pub mod proto;
pub mod state;
pub mod transport;
pub mod work_queue;
