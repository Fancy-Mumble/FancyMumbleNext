//! Key management for persistent chat.
//!
//! Provides the [`KeyManager`] struct which holds all cryptographic
//! state for a local participant: identity keys, known peer keys,
//! epoch keys, archive keys, and TOFU trust state.

mod consensus;
mod crypto;
mod exchange;
pub mod identity;
mod peer;
mod trust;
pub mod types;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use crate::persistent::encryption::{Encryptor, HkdfSha256Deriver, KeyDeriver, XChaChaEncryptor};
use crate::persistent::{KeyTrustLevel, PersistenceMode};

// Re-export public items for external consumers.
pub use identity::{CryptoIdentity, SeedIdentity};
pub use types::{
    ChannelKey, CustodianPinState, EncryptedPayload, EpochCandidate, EpochKey, PeerKeyRecord,
    channel_key_fingerprint,
};

use types::{ChannelKey as CK, ConsensusCollector, CustodianPinState as CPS, DEFAULT_MAX_REQUESTS};

// ---- KeyManager struct ----------------------------------------------

/// Central key management coordinator for persistent chat.
///
/// Holds the local identity, known peer keys, epoch/archive keys,
/// consensus collectors, and TOFU trust state for all channels.
pub struct KeyManager {
    /// Local cryptographic identity (signing + DH).
    pub(super) identity: Box<dyn CryptoIdentity>,
    /// AEAD encryptor (XChaCha20-Poly1305).
    pub(super) encryptor: Box<dyn Encryptor>,
    /// Key derivation function (HKDF-SHA256).
    pub(super) deriver: Box<dyn KeyDeriver>,

    /// Known peer public keys, keyed by `cert_hash`.
    pub(super) peer_keys: HashMap<String, PeerKeyRecord>,

    /// Epoch keys per channel: `channel_id -> epoch -> (EpochKey, trust)`.
    pub(super) epoch_keys: HashMap<u32, BTreeMap<u32, (EpochKey, KeyTrustLevel)>>,
    /// Archive keys per channel: `channel_id -> (ChannelKey, trust)`.
    pub(super) archive_keys: HashMap<u32, (ChannelKey, KeyTrustLevel)>,

    /// Number of key requests processed in this connection.
    pub(super) requests_processed: u32,
    /// Maximum key requests per connection (section 7.3).
    pub(super) max_requests_per_connection: u32,

    /// Active consensus collectors: `request_id -> collector`.
    pub(super) pending_consensus: HashMap<String, ConsensusCollector>,

    /// Channel originator cert hashes: `channel_id -> cert_hash`.
    pub(super) channel_originators: HashMap<u32, String>,

    /// Trust-on-first-use pinned custodian lists: `channel_id -> state`.
    pub(super) pinned_custodians: HashMap<u32, CPS>,

    /// Pending epoch candidates for fork resolution: `(channel_id, epoch) -> candidates`.
    pub(super) pending_epoch_candidates: HashMap<(u32, u32), Vec<EpochCandidate>>,

    /// Set of cert hashes that have known keys for each channel.
    pub(super) key_holders: HashMap<u32, HashSet<String>>,
}

impl std::fmt::Debug for KeyManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyManager")
            .field("peer_keys", &self.peer_keys.keys().collect::<Vec<_>>())
            .field(
                "epoch_keys_channels",
                &self.epoch_keys.keys().collect::<Vec<_>>(),
            )
            .field(
                "archive_keys_channels",
                &self.archive_keys.keys().collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

// ---- Constructors ---------------------------------------------------

impl KeyManager {
    /// Create a new key manager with default crypto implementations.
    pub fn new(identity: Box<dyn CryptoIdentity>) -> Self {
        Self::with_crypto(
            identity,
            Box::new(XChaChaEncryptor),
            Box::new(HkdfSha256Deriver),
        )
    }

    /// Create a key manager with custom crypto implementations (for testing).
    pub fn with_crypto(
        identity: Box<dyn CryptoIdentity>,
        encryptor: Box<dyn Encryptor>,
        deriver: Box<dyn KeyDeriver>,
    ) -> Self {
        Self {
            identity,
            encryptor,
            deriver,
            peer_keys: HashMap::new(),
            epoch_keys: HashMap::new(),
            archive_keys: HashMap::new(),
            requests_processed: 0,
            max_requests_per_connection: DEFAULT_MAX_REQUESTS,
            pending_consensus: HashMap::new(),
            channel_originators: HashMap::new(),
            pinned_custodians: HashMap::new(),
            pending_epoch_candidates: HashMap::new(),
            key_holders: HashMap::new(),
        }
    }
}

// ---- Basic accessors / mutators -------------------------------------

impl KeyManager {
    /// Whether we hold a key for the channel in the given mode.
    pub fn has_key(&self, channel_id: u32, mode: PersistenceMode) -> bool {
        match mode {
            PersistenceMode::PostJoin => self.epoch_keys.contains_key(&channel_id),
            PersistenceMode::FullArchive => self.archive_keys.contains_key(&channel_id),
            _ => false,
        }
    }

    /// Get the archive key bytes and trust level for a channel (if any).
    pub fn get_archive_key(&self, channel_id: u32) -> Option<([u8; 32], KeyTrustLevel)> {
        self.archive_keys
            .get(&channel_id)
            .map(|(ck, trust)| (ck.key, *trust))
    }

    /// Get the originator cert hash for a channel (if set).
    pub fn get_channel_originator(&self, channel_id: u32) -> Option<&str> {
        self.channel_originators.get(&channel_id).map(String::as_str)
    }

    /// Get the set of key holders for a channel.
    pub fn key_holders(&self, channel_id: u32) -> &HashSet<String> {
        static EMPTY: LazyLock<HashSet<String>> = LazyLock::new(HashSet::new);
        self.key_holders.get(&channel_id).unwrap_or(&EMPTY)
    }

    /// Record that a cert hash holds a key for a channel.
    pub fn record_key_holder(&mut self, channel_id: u32, cert_hash: String) {
        let _ = self
            .key_holders
            .entry(channel_id)
            .or_default()
            .insert(cert_hash);
    }

    /// Remove all keys and state for a channel.
    pub fn remove_channel(&mut self, channel_id: u32) {
        let _ = self.epoch_keys.remove(&channel_id);
        let _ = self.archive_keys.remove(&channel_id);
        let _ = self.channel_originators.remove(&channel_id);
        let _ = self.pinned_custodians.remove(&channel_id);
        let _ = self.key_holders.remove(&channel_id);
    }

    /// Compute `HMAC-SHA256(channel_key, challenge)` to prove possession of
    /// the archive key for the given channel.
    ///
    /// Returns `None` if no archive key is stored for `channel_id`.
    pub fn compute_challenge_proof(&self, channel_id: u32, challenge: &[u8]) -> Option<[u8; 32]> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let (key, _trust) = self.archive_keys.get(&channel_id)?;
        #[allow(clippy::expect_used, reason = "HMAC-SHA256 accepts any key length; InvalidLength is unreachable")]
        let mut mac = Hmac::<Sha256>::new_from_slice(&key.key)
            .expect("HMAC-SHA256 accepts any key size");
        mac.update(challenge);
        let result = mac.finalize();
        Some(result.into_bytes().into())
    }

    /// Our X25519 DH public key bytes.
    pub fn dh_public_bytes(&self) -> [u8; 32] {
        *self.identity.dh_public_key().as_bytes()
    }

    /// Our Ed25519 signing public key bytes.
    pub fn signing_public_bytes(&self) -> [u8; 32] {
        self.identity.signing_public_key().to_bytes()
    }

    /// Store an epoch key for a (channel, epoch).
    pub fn store_epoch_key(
        &mut self,
        channel_id: u32,
        epoch: u32,
        key: [u8; 32],
        trust: KeyTrustLevel,
    ) {
        let _ = self
            .epoch_keys
            .entry(channel_id)
            .or_default()
            .insert(epoch, (EpochKey::new(key), trust));
    }

    /// Store an archive key for a channel.
    pub fn store_archive_key(
        &mut self,
        channel_id: u32,
        key: [u8; 32],
        trust: KeyTrustLevel,
    ) {
        let _ = self
            .archive_keys
            .insert(channel_id, (CK { key }, trust));
    }

    /// Set the originator cert hash for a channel.
    pub fn set_channel_originator(&mut self, channel_id: u32, cert_hash: String) {
        let _ = self.channel_originators.insert(channel_id, cert_hash);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, reason = "unwrap/expect acceptable in test code")]
    use super::identity::SeedIdentity;
    use super::KeyManager;

    fn make_key_manager() -> KeyManager {
        let identity = SeedIdentity::from_seed(&[0xAA; 32]).unwrap();
        KeyManager::new(Box::new(identity))
    }

    #[test]
    fn challenge_proof_deterministic() {
        let mut km = make_key_manager();
        km.store_archive_key(1, [0x42; 32], crate::persistent::KeyTrustLevel::Unverified);
        let challenge = b"test challenge";
        let proof1 = km.compute_challenge_proof(1, challenge);
        let proof2 = km.compute_challenge_proof(1, challenge);
        assert_eq!(proof1, proof2);
        assert!(proof1.is_some());
    }

    #[test]
    fn challenge_proof_none_without_key() {
        let km = make_key_manager();
        assert!(km.compute_challenge_proof(1, b"challenge").is_none());
    }

    #[test]
    fn challenge_proof_differs_for_different_challenges() {
        let mut km = make_key_manager();
        km.store_archive_key(1, [0x42; 32], crate::persistent::KeyTrustLevel::Unverified);
        let p1 = km.compute_challenge_proof(1, b"challenge A");
        let p2 = km.compute_challenge_proof(1, b"challenge B");
        assert_ne!(p1, p2);
    }

    #[test]
    fn challenge_proof_returns_32_bytes() {
        let mut km = make_key_manager();
        km.store_archive_key(1, [0x55; 32], crate::persistent::KeyTrustLevel::Verified);
        let proof = km.compute_challenge_proof(1, b"server_challenge").unwrap();
        assert_eq!(proof.len(), 32);
    }

    #[test]
    fn challenge_proof_differs_by_key() {
        let mut km1 = make_key_manager();
        km1.store_archive_key(1, [0x11; 32], crate::persistent::KeyTrustLevel::Verified);

        let mut km2 = make_key_manager();
        km2.store_archive_key(1, [0x22; 32], crate::persistent::KeyTrustLevel::Verified);

        let challenge = b"same_challenge";
        let p1 = km1.compute_challenge_proof(1, challenge).unwrap();
        let p2 = km2.compute_challenge_proof(1, challenge).unwrap();
        assert_ne!(p1, p2, "different keys must produce different proofs");
    }

    #[test]
    fn challenge_proof_wrong_channel_returns_none() {
        let mut km = make_key_manager();
        km.store_archive_key(1, [0x55; 32], crate::persistent::KeyTrustLevel::Verified);
        assert!(km.compute_challenge_proof(2, b"challenge").is_none());
    }

    #[test]
    fn challenge_proof_after_remove_channel_returns_none() {
        let mut km = make_key_manager();
        km.store_archive_key(1, [0x55; 32], crate::persistent::KeyTrustLevel::Verified);
        assert!(km.compute_challenge_proof(1, b"c").is_some());
        km.remove_channel(1);
        assert!(km.compute_challenge_proof(1, b"c").is_none());
    }

    #[test]
    fn challenge_proof_same_key_different_managers_match() {
        // Two separate KeyManagers with the same archive key must produce
        // identical proofs for the same challenge. This simulates two clients
        // who received the same shared key.
        let key = [0xAB; 32];
        let challenge = b"server_nonce_12345";

        let mut km_a = make_key_manager();
        km_a.store_archive_key(5, key, crate::persistent::KeyTrustLevel::Verified);

        let mut km_b = {
            let identity = SeedIdentity::from_seed(&[0xBB; 32]).unwrap();
            KeyManager::new(Box::new(identity))
        };
        km_b.store_archive_key(5, key, crate::persistent::KeyTrustLevel::Verified);

        let pa = km_a.compute_challenge_proof(5, challenge).unwrap();
        let pb = km_b.compute_challenge_proof(5, challenge).unwrap();
        assert_eq!(pa, pb, "same key + same challenge must yield the same proof");
    }

    /// Regression: `compute_challenge_proof` must use HMAC-SHA256 keyed by the
    /// archive key, NOT a plain SHA-256 hash of identity keys or other data.
    /// A previous refactor accidentally replaced HMAC with SHA-256(`dh_public`
    /// || `signing_public` || `channel_id` || challenge) which produced a proof
    /// the server could not verify.
    #[test]
    fn challenge_proof_is_hmac_sha256_of_archive_key() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let archive_key = [0x99; 32];
        let challenge = b"regression_test_challenge";

        let mut km = make_key_manager();
        km.store_archive_key(7, archive_key, crate::persistent::KeyTrustLevel::Verified);

        let proof = km.compute_challenge_proof(7, challenge).unwrap();

        // Compute the expected HMAC-SHA256 independently.
        let mut mac = Hmac::<Sha256>::new_from_slice(&archive_key)
            .expect("HMAC-SHA256 accepts any key size");
        mac.update(challenge);
        let expected: [u8; 32] = mac.finalize().into_bytes().into();

        assert_eq!(
            proof, expected,
            "proof must be HMAC-SHA256(archive_key, challenge)"
        );
    }

    /// Regression: the proof must NOT depend on the identity keys. Two
    /// managers with different identities but the same archive key must
    /// produce identical proofs (i.e. identity keys are not mixed in).
    #[test]
    fn challenge_proof_independent_of_identity_keys() {
        let archive_key = [0x77; 32];
        let challenge = b"identity_independence_check";

        let mut km_a = {
            let id = SeedIdentity::from_seed(&[0x01; 32]).unwrap();
            KeyManager::new(Box::new(id))
        };
        km_a.store_archive_key(3, archive_key, crate::persistent::KeyTrustLevel::Verified);

        let mut km_b = {
            let id = SeedIdentity::from_seed(&[0x02; 32]).unwrap();
            KeyManager::new(Box::new(id))
        };
        km_b.store_archive_key(3, archive_key, crate::persistent::KeyTrustLevel::Verified);

        // Different identity seeds = different DH/signing keys, but same
        // archive key. Proofs must still match.
        assert_ne!(km_a.dh_public_bytes(), km_b.dh_public_bytes());
        assert_ne!(km_a.signing_public_bytes(), km_b.signing_public_bytes());

        let pa = km_a.compute_challenge_proof(3, challenge).unwrap();
        let pb = km_b.compute_challenge_proof(3, challenge).unwrap();
        assert_eq!(
            pa, pb,
            "proof must depend only on the archive key, not on identity keys"
        );
    }

    #[test]
    fn remove_channel_clears_all_state() {
        let mut km = make_key_manager();
        km.store_epoch_key(1, 0, [0; 32], crate::persistent::KeyTrustLevel::Unverified);
        km.store_archive_key(1, [0; 32], crate::persistent::KeyTrustLevel::Unverified);
        km.set_channel_originator(1, "abc".into());
        km.record_key_holder(1, "abc".into());
        assert!(km.has_key(1, crate::persistent::PersistenceMode::PostJoin));

        km.remove_channel(1);
        assert!(!km.has_key(1, crate::persistent::PersistenceMode::PostJoin));
        assert!(km.get_channel_originator(1).is_none());
        assert!(km.key_holders(1).is_empty());
    }
}
