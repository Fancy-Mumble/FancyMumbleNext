//! Signal Protocol V1 integration via dynamic library.
//!
//! Loads `signal_bridge.dll` / `libsignal_bridge.so` / `libsignal_bridge.dylib`
//! at runtime to keep the AGPL-licensed libsignal-protocol isolated from
//! the MIT-licensed codebase.
//!
//! Uses Signal's Sender Key mechanism for group encryption:
//! each sender creates a distribution message per channel, recipients
//! process it, then messages are encrypted/decrypted per-sender.

mod bridge;

pub use bridge::SignalBridge;
