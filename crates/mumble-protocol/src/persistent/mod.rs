//! Persistent encrypted chat for Fancy Mumble.
//!
//! This module implements the client-side architecture for persistent,
//! end-to-end encrypted chat history. All communication flows through
//! `PluginDataTransmission` with `dataID` values prefixed `fancy-pchat-`.
//!
//! Core types: [`PersistenceMode`], [`StoredMessage`], [`MessageRange`].
//! Provider trait: [`MessageProvider`] (in [`provider`]).

pub mod config;
pub mod encryption;
pub mod keys;
pub mod provider;
pub mod wire;

use serde::{Deserialize, Serialize};

// ---- Data ID constants for PluginDataTransmission -------------------

/// `dataID` for encrypted message storage.
pub const DATA_ID_MSG: &str = "fancy-pchat-msg";
/// `dataID` for stored message delivery (server to client).
pub const DATA_ID_MSG_DELIVER: &str = "fancy-pchat-msg-deliver";
/// `dataID` for fetch history request.
pub const DATA_ID_FETCH: &str = "fancy-pchat-fetch";
/// `dataID` for fetch history response.
pub const DATA_ID_FETCH_RESP: &str = "fancy-pchat-fetch-resp";
/// `dataID` for peer-to-peer key exchange.
pub const DATA_ID_KEY_EXCHANGE: &str = "fancy-pchat-key-exchange";
/// `dataID` for public key announcement.
pub const DATA_ID_KEY_ANNOUNCE: &str = "fancy-pchat-key-announce";
/// `dataID` for server-broadcast key request.
pub const DATA_ID_KEY_REQUEST: &str = "fancy-pchat-key-request";
/// `dataID` for key custodian countersignature.
pub const DATA_ID_EPOCH_COUNTERSIG: &str = "fancy-pchat-epoch-countersig";
/// `dataID` for server storage acknowledgement.
pub const DATA_ID_ACK: &str = "fancy-pchat-ack";

// ---- Core domain types ----------------------------------------------

/// Persistence mode for a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PersistenceMode {
    /// No persistence. Standard volatile Mumble chat.
    None,
    /// Messages accessible from the moment a user first joined.
    PostJoin,
    /// All stored messages accessible to any channel member.
    FullArchive,
    /// Future: server stores plaintext (or server-encrypted) messages.
    /// No client-side key management. See design doc section 3.1.
    ServerManaged,
}

impl PersistenceMode {
    /// Parse from the protobuf `pchat_mode` enum value (prost encodes
    /// proto2 enums as `i32`).
    #[must_use]
    pub fn from_proto(value: i32) -> Self {
        match value {
            1 => Self::PostJoin,
            2 => Self::FullArchive,
            3 => Self::ServerManaged,
            _ => Self::None,
        }
    }

    /// Convert to the protobuf `pchat_mode` enum value.
    #[must_use]
    pub fn to_proto(self) -> i32 {
        match self {
            Self::None => 0,
            Self::PostJoin => 1,
            Self::FullArchive => 2,
            Self::ServerManaged => 3,
        }
    }

    /// Mode string used in wire format payloads.
    #[must_use]
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::PostJoin => "POST_JOIN",
            Self::FullArchive => "FULL_ARCHIVE",
            Self::ServerManaged => "SERVER_MANAGED",
        }
    }

    /// Parse from wire format string.
    #[must_use]
    pub fn from_wire_str(s: &str) -> Self {
        match s {
            "POST_JOIN" => Self::PostJoin,
            "FULL_ARCHIVE" => Self::FullArchive,
            "SERVER_MANAGED" => Self::ServerManaged,
            _ => Self::None,
        }
    }

    /// Whether this mode uses client-side E2E encryption.
    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        matches!(self, Self::PostJoin | Self::FullArchive)
    }
}

/// Range for message queries.
#[derive(Debug, Clone)]
pub enum MessageRange {
    /// Latest N messages.
    Latest(usize),
    /// Messages before a cursor (pagination backwards).
    Before {
        message_id: String,
        limit: usize,
    },
    /// Messages after a cursor (pagination forwards).
    After {
        message_id: String,
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
    fn persistence_mode_proto_roundtrip() {
        for mode in [
            PersistenceMode::None,
            PersistenceMode::PostJoin,
            PersistenceMode::FullArchive,
            PersistenceMode::ServerManaged,
        ] {
            assert_eq!(PersistenceMode::from_proto(mode.to_proto()), mode);
        }
    }

    #[test]
    fn persistence_mode_wire_str_roundtrip() {
        for mode in [
            PersistenceMode::None,
            PersistenceMode::PostJoin,
            PersistenceMode::FullArchive,
            PersistenceMode::ServerManaged,
        ] {
            assert_eq!(PersistenceMode::from_wire_str(mode.as_wire_str()), mode);
        }
    }

    #[test]
    fn persistence_mode_is_encrypted() {
        assert!(!PersistenceMode::None.is_encrypted());
        assert!(PersistenceMode::PostJoin.is_encrypted());
        assert!(PersistenceMode::FullArchive.is_encrypted());
        assert!(!PersistenceMode::ServerManaged.is_encrypted());
    }

    #[test]
    fn unknown_proto_value_defaults_to_none() {
        assert_eq!(PersistenceMode::from_proto(99), PersistenceMode::None);
    }

    #[test]
    fn unknown_wire_str_defaults_to_none() {
        assert_eq!(PersistenceMode::from_wire_str("INVALID"), PersistenceMode::None);
    }
}
