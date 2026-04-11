//! Supporting types, constants, and data structures for key management.

use std::collections::HashMap;
use std::time::Instant;

use x25519_dalek::PublicKey as X25519PublicKey;

use crate::persistent::encryption::epoch_fingerprint;
use crate::persistent::PchatProtocol;

// ---- Constants ------------------------------------------------------

/// Current algorithm version for key announces and exchanges.
pub const ALGORITHM_VERSION: u8 = 1;

/// Maximum key requests processed per connection (section 7.3).
pub(super) const DEFAULT_MAX_REQUESTS: u32 = 50;

/// Consensus collection window duration (section 5.3).
pub(super) const CONSENSUS_WINDOW_SECS: u64 = 10;

/// Countersignature freshness window (section 5.6.4).
pub(super) const COUNTERSIG_FRESHNESS_MS: u64 = 5 * 60 * 1000; // 5 minutes

/// Key exchange timestamp freshness window (section 6.6).
pub(super) const KEY_EXCHANGE_FRESHNESS_MS: u64 = 5 * 60 * 1000; // 5 minutes

// ---- Supporting types -----------------------------------------------

/// An epoch key with its chain ratchet state.
#[derive(Debug, Clone)]
pub struct EpochKey {
    /// The raw 32-byte epoch key.
    pub key: [u8; 32],
    /// Current chain index (how far the ratchet has advanced).
    pub chain_index: u32,
    /// Current chain key (ratcheted from epoch key).
    pub current_chain_key: [u8; 32],
}

impl EpochKey {
    /// Create a new epoch key with chain index 0.
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            current_chain_key: key,
            key,
            chain_index: 0,
        }
    }

    /// Compute the fingerprint for this epoch key.
    pub fn fingerprint(&self) -> [u8; 8] {
        epoch_fingerprint(&self.key)
    }
}

/// A static channel key (`FULL_ARCHIVE` mode).
#[derive(Debug, Clone)]
pub struct ChannelKey {
    /// The raw 32-byte channel key.
    pub key: [u8; 32],
}

impl ChannelKey {
    /// Compute the fingerprint for this channel key.
    pub fn fingerprint(&self) -> [u8; 8] {
        epoch_fingerprint(&self.key)
    }
}

/// Cached peer identity keys with anti-rollback timestamp.
#[derive(Debug, Clone)]
pub struct PeerKeyRecord {
    /// Algorithm version from the key announcement (section 6.8).
    pub algorithm_version: u8,
    /// Key-agreement public key (X25519 for v1).
    pub dh_public: X25519PublicKey,
    /// Signature public key (Ed25519 for v1).
    pub signing_public: ed25519_dalek::VerifyingKey,
    /// Highest observed announce timestamp (anti-rollback).
    pub highest_announce_ts: u64,
}

/// A candidate epoch key received during the fork-resolution window.
#[derive(Debug, Clone)]
pub struct EpochCandidate {
    /// Cert hash of the sender who generated this epoch key.
    pub sender_hash: String,
    /// The epoch key itself.
    pub epoch_key: EpochKey,
    /// `parent_fingerprint` from the key-exchange.
    pub parent_fingerprint: [u8; 8],
    /// `epoch_fingerprint` from the key-exchange.
    pub epoch_fingerprint: [u8; 8],
    /// When this candidate was received.
    pub received_at: Instant,
}

/// Collects key-exchange responses during the consensus window.
#[derive(Debug, Clone)]
pub struct ConsensusCollector {
    /// When the collection window started (first response received).
    pub window_start: Instant,
    /// Collected responses: `sender_hash` -> decrypted key bytes.
    pub responses: HashMap<String, Vec<u8>>,
    /// Request timestamp from `fancy-pchat-key-request`.
    pub request_timestamp: u64,
    /// Number of Fancy Mumble v2+ members observed in the channel.
    pub observed_members: u32,
}

impl ConsensusCollector {
    /// Whether the 10-second collection window has elapsed.
    pub fn window_elapsed(&self) -> bool {
        self.window_start.elapsed().as_secs() >= CONSENSUS_WINDOW_SECS
    }
}

/// Tracks TOFU state of a channel's key custodian list.
#[derive(Debug, Clone)]
pub struct CustodianPinState {
    /// Currently trusted custodian cert hashes.
    pub pinned: Vec<String>,
    /// Whether the user has explicitly confirmed this list.
    pub confirmed: bool,
    /// Pending custodian list update awaiting user acceptance.
    pub pending_update: Option<Vec<String>>,
}

impl CustodianPinState {
    /// Create with an empty pinned list (no custodians).
    pub fn empty() -> Self {
        Self {
            pinned: Vec::new(),
            confirmed: true, // empty list needs no confirmation
            pending_update: None,
        }
    }

    /// Create from a first-observed custodian list.
    /// The list is auto-confirmed when empty (no custodians means no confirmation needed).
    pub fn first_observation(custodians: Vec<String>) -> Self {
        let confirmed = custodians.is_empty();
        Self {
            pinned: custodians,
            confirmed,
            pending_update: None,
        }
    }
}

// ---- Encrypted payload wrapper --------------------------------------

/// The encrypted payload returned by [`super::KeyManager::encrypt`].
#[derive(Debug, Clone)]
pub struct EncryptedPayload {
    /// Version byte + nonce + AEAD ciphertext + tag.
    pub ciphertext: Vec<u8>,
    /// Epoch number (`POST_JOIN`).
    pub epoch: Option<u32>,
    /// Chain index within epoch (`POST_JOIN`).
    pub chain_index: Option<u32>,
    /// `SHA-256(key)[0..8]`.
    pub epoch_fingerprint: [u8; 8],
}

// ---- Key fingerprint computation (section 5.6.2) --------------------

/// Compute the full channel key fingerprint for verification.
///
/// `full_fingerprint = SHA-256(channel_key || channel_id(4B BE) || mode(1B))`
pub fn channel_key_fingerprint(key: &[u8], channel_id: u32, protocol: PchatProtocol) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(key);
    hasher.update(channel_id.to_be_bytes());
    hasher.update([protocol.to_proto() as u8]);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custodian_pin_first_observation_empty() {
        let state = CustodianPinState::first_observation(vec![]);
        assert!(state.confirmed);
        assert!(state.pinned.is_empty());
    }

    #[test]
    fn custodian_pin_first_observation_populated() {
        let state = CustodianPinState::first_observation(vec!["abc".into()]);
        assert!(!state.confirmed);
        assert_eq!(state.pinned, vec!["abc"]);
    }

    #[test]
    fn channel_key_fingerprint_deterministic() {
        let key = [0xBB; 32];
        let fp1 = channel_key_fingerprint(&key, 1, PchatProtocol::FancyV1FullArchive);
        let fp2 = channel_key_fingerprint(&key, 1, PchatProtocol::FancyV1FullArchive);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn channel_key_fingerprint_differs_by_channel() {
        let key = [0xBB; 32];
        let fp1 = channel_key_fingerprint(&key, 1, PchatProtocol::FancyV1FullArchive);
        let fp2 = channel_key_fingerprint(&key, 2, PchatProtocol::FancyV1FullArchive);
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn channel_key_fingerprint_differs_by_mode() {
        let key = [0xBB; 32];
        let fp1 = channel_key_fingerprint(&key, 1, PchatProtocol::FancyV1FullArchive);
        let fp2 = channel_key_fingerprint(&key, 1, PchatProtocol::SignalV1);
        assert_ne!(fp1, fp2);
    }
}
