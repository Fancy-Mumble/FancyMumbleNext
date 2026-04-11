//! Persistent encrypted chat for Fancy Mumble.
//!
//! This module implements the client-side architecture for persistent,
//! end-to-end encrypted chat history. Communication uses native
//! protobuf messages (`PchatMessage`, `PchatFetch`, etc.) defined in
//! `Mumble.proto`.
//!
//! Core types: [`PchatProtocol`], [`StoredMessage`], [`MessageRange`].
//! Provider trait: [`MessageProvider`] (in [`provider`]).

pub mod config;
pub mod encryption;
pub mod keys;
pub mod protocol;
pub mod provider;
pub mod wire;

use serde::{Deserialize, Serialize};

// ---- Core domain types ----------------------------------------------

// Re-export the unified protocol enum from state.rs so persistent/
// sub-modules can use `crate::persistent::PchatProtocol`.
pub use crate::state::PchatProtocol;

// Extension methods for wire serialization (kept here because they
// are a persistent-chat concern, not a core state concern).
impl PchatProtocol {
    /// Protocol string used in wire format payloads (`MessagePack`).
    #[must_use]
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::FancyV1FullArchive => "FANCY_V1_FULL_ARCHIVE",
            Self::SignalV1 => "SIGNAL_V1",
        }
    }

    /// Parse from wire format string.
    ///
    /// Accepts both current names (`"FANCY_V1_POST_JOIN"`,
    /// `"FANCY_V1_FULL_ARCHIVE"`) and legacy names (`"POST_JOIN"`,
    /// `"FULL_ARCHIVE"`) for backward compatibility with stored messages
    /// and older clients/servers.
    #[must_use]
    pub fn from_wire_str(s: &str) -> Self {
        match s {
            "FANCY_V1_FULL_ARCHIVE" | "FULL_ARCHIVE" => Self::FancyV1FullArchive,
            "SIGNAL_V1" => Self::SignalV1,
            _ => Self::None,
        }
    }
}

/// Range for message queries.
#[derive(Debug, Clone)]
pub enum MessageRange {
    /// Latest N messages.
    Latest(usize),
    /// Messages before a cursor (pagination backwards).
    Before {
        /// Message ID cursor to paginate before.
        message_id: String,
        /// Maximum number of messages to return.
        limit: usize,
    },
    /// Messages after a cursor (pagination forwards).
    After {
        /// Message ID cursor to paginate after.
        message_id: String,
        /// Maximum number of messages to return.
        limit: usize,
    },
}

/// A message as stored/retrieved by any [`MessageProvider`](provider::MessageProvider).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    /// Unique message identifier (UUID v4).
    pub message_id: String,
    /// Target channel.
    pub channel_id: u32,
    /// Unix epoch milliseconds.
    pub timestamp: u64,
    /// Sender's TLS certificate hash (identity).
    pub sender_hash: String,
    /// Display name at send time.
    pub sender_name: String,
    /// Message body (HTML). Plaintext after decryption.
    pub body: String,
    /// Whether the body is still ciphertext (needs decryption).
    pub encrypted: bool,
    /// Epoch number (`POST_JOIN` only).
    pub epoch: Option<u32>,
    /// Chain ratchet index within the epoch (`POST_JOIN` only).
    pub chain_index: Option<u32>,
    /// If set, this message replaces a previous message with the
    /// given ID (epoch fork re-send). See design doc section 6.2.
    pub replaces_id: Option<String>,
}

/// Trust level for a received encryption key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyTrustLevel {
    /// Key fingerprint confirmed via out-of-band comparison.
    ManuallyVerified,
    /// Multi-confirmed or validated by key custodian / countersignature.
    Verified,
    /// Single source, accepted on first use (TOFU).
    Unverified,
    /// Conflicting keys received from different members.
    Disputed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pchat_protocol_proto_roundtrip() {
        for protocol in [
            PchatProtocol::None,
            PchatProtocol::FancyV1FullArchive,
            PchatProtocol::SignalV1,
        ] {
            assert_eq!(PchatProtocol::from_proto(protocol.to_proto()), protocol);
        }
    }

    #[test]
    fn pchat_protocol_wire_str_roundtrip() {
        for protocol in [
            PchatProtocol::None,
            PchatProtocol::FancyV1FullArchive,
            PchatProtocol::SignalV1,
        ] {
            assert_eq!(PchatProtocol::from_wire_str(protocol.as_wire_str()), protocol);
        }
    }

    #[test]
    fn pchat_protocol_is_encrypted() {
        assert!(!PchatProtocol::None.is_encrypted());
        assert!(PchatProtocol::FancyV1FullArchive.is_encrypted());
        assert!(PchatProtocol::SignalV1.is_encrypted());
    }

    #[test]
    fn unknown_proto_value_defaults_to_none() {
        assert_eq!(PchatProtocol::from_proto(99), PchatProtocol::None);
    }

    #[test]
    fn unknown_wire_str_defaults_to_none() {
        assert_eq!(PchatProtocol::from_wire_str("INVALID"), PchatProtocol::None);
    }

    #[test]
    fn protocol_version_is_correct() {
        assert_eq!(PchatProtocol::None.protocol_version(), None);
        assert_eq!(PchatProtocol::FancyV1FullArchive.protocol_version(), Some(1));
        assert_eq!(PchatProtocol::SignalV1.protocol_version(), Some(2));
    }
}
