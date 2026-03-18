//! Key management for persistent encrypted chat.
//!
//! Provides [`KeyManager`] and supporting types for identity key pairs,
//! peer key tracking, epoch/channel key storage, consensus collection,
//! and key custodian TOFU pinning.
//!
//! The key manager delegates cryptographic operations to trait objects
//! ([`Encryptor`], [`KeyDeriver`]) so that alternative algorithms or
//! test mocks can be injected.

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use ed25519_dalek::{Signer, Verifier};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

use crate::error::{Error, Result};
use crate::persistent::encryption::{
    self, build_countersig_data, build_key_exchange_signed_data, epoch_fingerprint, Encryptor,
    HkdfSha256Deriver, KeyDeriver, XChaChaEncryptor,
};
use crate::persistent::wire::{PchatKeyAnnounce, PchatKeyExchange, PchatKeyRequest};
use crate::persistent::{KeyTrustLevel, PersistenceMode, StoredMessage};

/// Current algorithm version for key announces and exchanges.
pub const ALGORITHM_VERSION: u8 = 1;

/// Maximum key requests processed per connection (section 7.3).
const DEFAULT_MAX_REQUESTS: u32 = 50;

/// Consensus collection window duration (section 5.3).
const CONSENSUS_WINDOW_SECS: u64 = 10;

/// Countersignature freshness window (section 5.6.4).
const COUNTERSIG_FRESHNESS_MS: u64 = 5 * 60 * 1000; // 5 minutes

/// Key exchange timestamp freshness window (section 6.6).
const KEY_EXCHANGE_FRESHNESS_MS: u64 = 5 * 60 * 1000; // 5 minutes

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
    pub key: [u8; 32],
}

impl ChannelKey {
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

/// The encrypted payload returned by [`KeyManager::encrypt`].
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
pub fn channel_key_fingerprint(key: &[u8], channel_id: u32, mode: PersistenceMode) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(key);
    hasher.update(channel_id.to_be_bytes());
    hasher.update([mode.to_proto() as u8]);
    hasher.finalize().into()
}

// ---- Identity trait -------------------------------------------------

/// Trait abstracting cryptographic identity operations.
///
/// Implementations provide signing, DH key agreement, and
/// public key export. This allows testing with deterministic keys.
pub trait CryptoIdentity: Send + Sync {
    /// Our X25519 public key for key agreement.
    fn dh_public_key(&self) -> X25519PublicKey;

    /// Our Ed25519 verifying (public) key for signature verification.
    fn signing_public_key(&self) -> ed25519_dalek::VerifyingKey;

    /// Perform X25519 Diffie-Hellman with a peer's public key.
    fn dh_agree(&self, peer_public: &X25519PublicKey) -> [u8; 32];

    /// Sign data with the Ed25519 private key.
    fn sign(&self, data: &[u8]) -> ed25519_dalek::Signature;
}

/// Production identity backed by a 32-byte seed.
pub struct SeedIdentity {
    dh_secret: X25519StaticSecret,
    dh_public: X25519PublicKey,
    signing_key: ed25519_dalek::SigningKey,
}

impl SeedIdentity {
    /// Derive identity from the 32-byte seed using HKDF.
    pub fn from_seed(seed: &[u8; 32]) -> Result<Self> {
        let deriver = HkdfSha256Deriver;

        let dh_bytes = deriver.derive(
            seed,
            encryption::HKDF_SALT_IDENTITY,
            encryption::HKDF_INFO_X25519,
        )?;
        let dh_secret = X25519StaticSecret::from(dh_bytes);
        let dh_public = X25519PublicKey::from(&dh_secret);

        let sign_bytes = deriver.derive(
            seed,
            encryption::HKDF_SALT_IDENTITY,
            encryption::HKDF_INFO_ED25519,
        )?;
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&sign_bytes);

        Ok(Self {
            dh_secret,
            dh_public,
            signing_key,
        })
    }
}

impl std::fmt::Debug for SeedIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeedIdentity")
            .field("dh_public", &self.dh_public.as_bytes())
            .finish_non_exhaustive()
    }
}

impl CryptoIdentity for SeedIdentity {
    fn dh_public_key(&self) -> X25519PublicKey {
        self.dh_public
    }

    fn signing_public_key(&self) -> ed25519_dalek::VerifyingKey {
        self.signing_key.verifying_key()
    }

    fn dh_agree(&self, peer_public: &X25519PublicKey) -> [u8; 32] {
        *self.dh_secret.diffie_hellman(peer_public).as_bytes()
    }

    fn sign(&self, data: &[u8]) -> ed25519_dalek::Signature {
        self.signing_key.sign(data)
    }
}

// ---- KeyManager -----------------------------------------------------

/// Central key manager for persistent chat encryption.
///
/// Manages identity keys, peer keys, epoch/channel keys, consensus
/// state, custodian TOFU, and epoch fork resolution.
pub struct KeyManager {
    /// Our cryptographic identity (signing + DH).
    identity: Box<dyn CryptoIdentity>,
    /// Encryptor for message-level AEAD.
    encryptor: Box<dyn Encryptor>,
    /// Key deriver for HKDF operations.
    deriver: Box<dyn KeyDeriver>,
    /// Known peer public keys: `cert_hash` -> record.
    peer_keys: HashMap<String, PeerKeyRecord>,
    /// `POST_JOIN` epoch keys: `channel_id` -> epoch -> (key, trust).
    epoch_keys: HashMap<u32, BTreeMap<u32, (EpochKey, KeyTrustLevel)>>,
    /// `FULL_ARCHIVE` channel keys: `channel_id` -> (key, trust).
    archive_keys: HashMap<u32, (ChannelKey, KeyTrustLevel)>,
    /// Key requests processed this connection.
    requests_processed: u32,
    /// Max key requests per connection.
    max_requests_per_connection: u32,
    /// Pending consensus collectors: `request_id` -> collector.
    pending_consensus: HashMap<String, ConsensusCollector>,
    /// Channel key originators: `channel_id` -> `cert_hash`.
    channel_originators: HashMap<u32, String>,
    /// Pinned custodian lists per channel.
    pinned_custodians: HashMap<u32, CustodianPinState>,
    /// Pending epoch fork candidates (`POST_JOIN`).
    /// Key: (`channel_id`, epoch) -> candidates.
    pending_epoch_candidates: HashMap<(u32, u32), Vec<EpochCandidate>>,
}

impl std::fmt::Debug for KeyManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyManager")
            .field("peer_keys_count", &self.peer_keys.len())
            .field("epoch_keys_count", &self.epoch_keys.len())
            .field("archive_keys_count", &self.archive_keys.len())
            .field("requests_processed", &self.requests_processed)
            .finish_non_exhaustive()
    }
}

impl KeyManager {
    /// Create a new `KeyManager` with the given identity.
    pub fn new(identity: Box<dyn CryptoIdentity>) -> Self {
        Self {
            identity,
            encryptor: Box::new(XChaChaEncryptor),
            deriver: Box::new(HkdfSha256Deriver),
            peer_keys: HashMap::new(),
            epoch_keys: HashMap::new(),
            archive_keys: HashMap::new(),
            requests_processed: 0,
            max_requests_per_connection: DEFAULT_MAX_REQUESTS,
            pending_consensus: HashMap::new(),
            channel_originators: HashMap::new(),
            pinned_custodians: HashMap::new(),
            pending_epoch_candidates: HashMap::new(),
        }
    }

    /// Returns `true` if we already hold a key for the given channel and mode.
    pub fn has_key(&self, channel_id: u32, mode: PersistenceMode) -> bool {
        match mode {
            PersistenceMode::PostJoin => self.epoch_keys.contains_key(&channel_id),
            PersistenceMode::FullArchive => self.archive_keys.contains_key(&channel_id),
            _ => false,
        }
    }

    /// Create with custom encryptor and deriver (useful for testing).
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
        }
    }

    // ---- Public key accessors ---------------------------------------

    /// Our X25519 public key bytes.
    pub fn dh_public_bytes(&self) -> [u8; 32] {
        *self.identity.dh_public_key().as_bytes()
    }

    /// Our Ed25519 verifying key bytes.
    pub fn signing_public_bytes(&self) -> [u8; 32] {
        self.identity.signing_public_key().to_bytes()
    }

    // ---- Peer key management ----------------------------------------

    /// Record a peer's public keys from a `fancy-pchat-key-announce`.
    ///
    /// Enforces anti-rollback: discards announcements with timestamp
    /// <= the known highest for this peer (section 6.8).
    pub fn record_peer_key(&mut self, announce: &PchatKeyAnnounce) -> Result<bool> {
        if announce.algorithm_version != ALGORITHM_VERSION {
            return Err(Error::InvalidState(format!(
                "unsupported algorithm_version: {}",
                announce.algorithm_version
            )));
        }

        if announce.identity_public.len() != 32 || announce.signing_public.len() != 32 {
            tracing::warn!(
                cert_hash = %announce.cert_hash,
                id_pub_len = announce.identity_public.len(),
                sign_pub_len = announce.signing_public.len(),
                sig_len = announce.signature.len(),
                "key-announce has invalid key lengths (expected 32, 32, 64) \
                 -- possible BLOB truncation by server DB"
            );
            return Err(Error::InvalidState("invalid key lengths".into()));
        }

        // Verify Ed25519 self-signature
        let signed_data = encryption::build_key_announce_signed_data(
            announce.algorithm_version,
            &announce.cert_hash,
            announce.timestamp,
            &announce.identity_public,
            &announce.signing_public,
        );

        let signing_bytes: [u8; 32] = announce.signing_public[..32]
            .try_into()
            .map_err(|_| Error::InvalidState("invalid signing key".into()))?;
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&signing_bytes)
            .map_err(|e| Error::InvalidState(format!("invalid Ed25519 key: {e}")))?;
        let signature = ed25519_dalek::Signature::from_slice(&announce.signature)
            .map_err(|e| Error::InvalidState(format!("invalid signature: {e}")))?;

        verifying_key
            .verify(&signed_data, &signature)
            .map_err(|e| {
                let sig_hex: String = announce.signature.iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                tracing::warn!(
                    cert_hash = %announce.cert_hash,
                    timestamp = announce.timestamp,
                    signed_data_len = signed_data.len(),
                    sig_hex,
                    "key-announce signature verification failed: {e}"
                );
                Error::InvalidState(format!("signature verification failed: {e}"))
            })?;

        // Anti-rollback check
        if let Some(existing) = self.peer_keys.get(&announce.cert_hash) {
            if announce.timestamp <= existing.highest_announce_ts {
                return Ok(false); // silently discard stale announcement
            }
        }

        let dh_bytes: [u8; 32] = announce.identity_public[..32]
            .try_into()
            .map_err(|_| Error::InvalidState("invalid DH key".into()))?;

        self.peer_keys.insert(
            announce.cert_hash.clone(),
            PeerKeyRecord {
                algorithm_version: announce.algorithm_version,
                dh_public: X25519PublicKey::from(dh_bytes),
                signing_public: verifying_key,
                highest_announce_ts: announce.timestamp,
            },
        );

        Ok(true)
    }

    /// Look up a peer's known keys.
    pub fn get_peer(&self, cert_hash: &str) -> Option<&PeerKeyRecord> {
        self.peer_keys.get(cert_hash)
    }

    // ---- Encryption / Decryption ------------------------------------

    /// Encrypt a message for the given mode and channel.
    pub fn encrypt(
        &mut self,
        mode: PersistenceMode,
        channel_id: u32,
        message_id: &str,
        timestamp: u64,
        plaintext: &[u8],
    ) -> Result<EncryptedPayload> {
        let uuid_bytes = encryption::uuid_to_bytes(message_id)?;
        let aad = encryption::build_aad(channel_id, &uuid_bytes, timestamp);

        match mode {
            PersistenceMode::PostJoin => {
                let epochs = self
                    .epoch_keys
                    .get_mut(&channel_id)
                    .ok_or_else(|| Error::InvalidState("no epoch keys for channel".into()))?;
                let (&current_epoch, entry) = epochs
                    .iter_mut()
                    .next_back()
                    .ok_or_else(|| Error::InvalidState("no current epoch".into()))?;
                let (epoch_key, _trust) = entry;

                let msg_key =
                    encryption::derive_message_key(&*self.deriver, &epoch_key.current_chain_key)?;
                let chain_index = epoch_key.chain_index;

                // Ratchet chain forward
                epoch_key.current_chain_key =
                    encryption::derive_chain_key(&*self.deriver, &epoch_key.current_chain_key)?;
                epoch_key.chain_index += 1;

                let ciphertext = self.encryptor.encrypt(&msg_key, plaintext, &aad)?;
                let fp = epoch_key.fingerprint();

                Ok(EncryptedPayload {
                    ciphertext,
                    epoch: Some(current_epoch),
                    chain_index: Some(chain_index),
                    epoch_fingerprint: fp,
                })
            }
            PersistenceMode::FullArchive => {
                let (channel_key, _trust) = self
                    .archive_keys
                    .get(&channel_id)
                    .ok_or_else(|| Error::InvalidState("no archive key for channel".into()))?;

                let ciphertext = self.encryptor.encrypt(&channel_key.key, plaintext, &aad)?;
                let fp = channel_key.fingerprint();

                Ok(EncryptedPayload {
                    ciphertext,
                    epoch: None,
                    chain_index: None,
                    epoch_fingerprint: fp,
                })
            }
            _ => Err(Error::InvalidState(format!(
                "cannot encrypt for mode {mode:?}"
            ))),
        }
    }

    /// Decrypt a message.
    pub fn decrypt(
        &self,
        mode: PersistenceMode,
        channel_id: u32,
        message_id: &str,
        timestamp: u64,
        payload: &EncryptedPayload,
    ) -> Result<Vec<u8>> {
        let uuid_bytes = encryption::uuid_to_bytes(message_id)?;
        let aad = encryption::build_aad(channel_id, &uuid_bytes, timestamp);

        match mode {
            PersistenceMode::PostJoin => {
                let epoch = payload
                    .epoch
                    .ok_or_else(|| Error::InvalidState("missing epoch for POST_JOIN".into()))?;
                let chain_idx = payload
                    .chain_index
                    .ok_or_else(|| Error::InvalidState("missing chain_index".into()))?;

                let epochs = self
                    .epoch_keys
                    .get(&channel_id)
                    .ok_or_else(|| Error::InvalidState("no epoch keys for channel".into()))?;
                let (epoch_key, _trust) = epochs
                    .get(&epoch)
                    .ok_or_else(|| Error::InvalidState(format!("unknown epoch: {epoch}")))?;

                // Re-derive the message key at the specified chain index
                let msg_key =
                    encryption::derive_key_at_index(&*self.deriver, &epoch_key.key, chain_idx)?;

                self.encryptor.decrypt(&msg_key, &payload.ciphertext, &aad)
            }
            PersistenceMode::FullArchive => {
                let (channel_key, _trust) = self
                    .archive_keys
                    .get(&channel_id)
                    .ok_or_else(|| Error::InvalidState("no archive key for channel".into()))?;

                self.encryptor
                    .decrypt(&channel_key.key, &payload.ciphertext, &aad)
            }
            _ => Err(Error::InvalidState(format!(
                "cannot decrypt for mode {mode:?}"
            ))),
        }
    }

    // ---- Key exchange signature verification ------------------------

    /// Verify the Ed25519 signature on a key-exchange payload.
    pub fn verify_key_exchange_signature(&self, exchange: &PchatKeyExchange) -> Result<()> {
        let peer = self.peer_keys.get(&exchange.sender_hash).ok_or_else(|| {
            Error::InvalidState(format!("unknown sender: {}", exchange.sender_hash))
        })?;

        if exchange.algorithm_version != peer.algorithm_version {
            return Err(Error::InvalidState(
                "algorithm_version mismatch with sender's announced version".into(),
            ));
        }

        let mode = PersistenceMode::from_wire_str(&exchange.mode);
        let signed_data = build_key_exchange_signed_data(
            exchange.algorithm_version,
            exchange.channel_id,
            &mode,
            exchange.epoch,
            &exchange.encrypted_key,
            &exchange.recipient_hash,
            exchange.request_id.as_deref(),
            exchange.timestamp,
        );

        let signature = ed25519_dalek::Signature::from_slice(&exchange.signature)
            .map_err(|e| Error::InvalidState(format!("invalid signature bytes: {e}")))?;

        peer.signing_public
            .verify(&signed_data, &signature)
            .map_err(|e| Error::InvalidState(format!("key-exchange signature invalid: {e}")))
    }

    // ---- Key exchange processing ------------------------------------

    /// Process an incoming key exchange message.
    ///
    /// 1. Verifies Ed25519 signature.
    /// 2. Checks timestamp freshness.
    /// 3. Decrypts the key via DH shared secret.
    /// 4. Verifies `epoch_fingerprint` matches.
    /// 5. For `POST_JOIN`: verifies `parent_fingerprint`, stores as candidate.
    /// 6. For `FULL_ARCHIVE`: adds to consensus collector.
    pub fn receive_key_exchange(
        &mut self,
        exchange: &PchatKeyExchange,
        request_timestamp: Option<u64>,
    ) -> Result<()> {
        // 1. Verify signature
        self.verify_key_exchange_signature(exchange)?;

        // 2. Timestamp freshness
        if let Some(req_ts) = request_timestamp {
            if exchange.timestamp < req_ts {
                return Err(Error::InvalidState(
                    "key-exchange timestamp before request".into(),
                ));
            }
            if exchange.timestamp > req_ts + KEY_EXCHANGE_FRESHNESS_MS {
                return Err(Error::InvalidState(
                    "key-exchange timestamp too far after request".into(),
                ));
            }
        }

        // 3. Decrypt the key via DH
        let peer = self
            .peer_keys
            .get(&exchange.sender_hash)
            .ok_or_else(|| Error::InvalidState("unknown sender".into()))?;

        let shared_secret = self.identity.dh_agree(&peer.dh_public);
        let decrypt_key = self
            .deriver
            .derive(&shared_secret, encryption::HKDF_SALT_IDENTITY, b"key-wrap")?;

        let decrypted_key_bytes = self
            .encryptor
            .decrypt(&decrypt_key, &exchange.encrypted_key, &[])?;

        if decrypted_key_bytes.len() != 32 {
            return Err(Error::InvalidState(format!(
                "decrypted key is {} bytes, expected 32",
                decrypted_key_bytes.len()
            )));
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&decrypted_key_bytes);

        // 4. Verify epoch_fingerprint
        let computed_fp = epoch_fingerprint(&key_bytes);
        if exchange.epoch_fingerprint.len() != 8 || computed_fp != exchange.epoch_fingerprint[..8] {
            return Err(Error::InvalidState(
                "epoch_fingerprint mismatch".into(),
            ));
        }

        let mode = PersistenceMode::from_wire_str(&exchange.mode);

        match mode {
            PersistenceMode::PostJoin => {
                // 5. Store as epoch candidate for fork resolution
                let mut parent_fp = [0u8; 8];
                if let Some(ref pfp) = exchange.parent_fingerprint {
                    if pfp.len() == 8 {
                        parent_fp.copy_from_slice(pfp);
                    }
                }

                let candidate = EpochCandidate {
                    sender_hash: exchange.sender_hash.clone(),
                    epoch_key: EpochKey::new(key_bytes),
                    parent_fingerprint: parent_fp,
                    epoch_fingerprint: computed_fp,
                    received_at: Instant::now(),
                };

                self.pending_epoch_candidates
                    .entry((exchange.channel_id, exchange.epoch))
                    .or_default()
                    .push(candidate);
            }
            PersistenceMode::FullArchive => {
                // 6. Add to consensus collector
                if let Some(ref request_id) = exchange.request_id {
                    let collector =
                        self.pending_consensus
                            .entry(request_id.clone())
                            .or_insert_with(|| ConsensusCollector {
                                window_start: Instant::now(),
                                responses: HashMap::new(),
                                request_timestamp: request_timestamp.unwrap_or(0),
                                observed_members: 0,
                            });
                    collector
                        .responses
                        .insert(exchange.sender_hash.clone(), key_bytes.to_vec());
                } else {
                    // Direct key acceptance (no request_id, e.g. key custodian shortcut)
                    self.archive_keys.insert(
                        exchange.channel_id,
                        (ChannelKey { key: key_bytes }, KeyTrustLevel::Unverified),
                    );
                }
            }
            _ => {
                return Err(Error::InvalidState(format!(
                    "unexpected mode in key-exchange: {mode:?}"
                )));
            }
        }

        // Check for inline countersignature
        if let (Some(ref countersig), Some(ref countersigner)) =
            (&exchange.countersignature, &exchange.countersigner_hash)
        {
            let parent_fp = exchange
                .parent_fingerprint
                .as_deref()
                .unwrap_or(&[0u8; 8]);
            let _ = self.verify_countersignature_internal(
                exchange.channel_id,
                exchange.epoch,
                &exchange.epoch_fingerprint,
                parent_fp,
                countersigner,
                &exchange.sender_hash,
                exchange.timestamp,
                countersig,
            );
        }

        Ok(())
    }

    // ---- Consensus evaluation ---------------------------------------

    /// Evaluate consensus after the 10-second collection window closes.
    ///
    /// Returns the resulting trust level and the accepted key bytes (if any).
    pub fn evaluate_consensus(
        &mut self,
        request_id: &str,
        channel_id: u32,
        key_custodians: &[String],
    ) -> Result<(KeyTrustLevel, Option<[u8; 32]>)> {
        let collector = self
            .pending_consensus
            .remove(request_id)
            .ok_or_else(|| Error::InvalidState("no consensus collector".into()))?;

        if collector.responses.is_empty() {
            return Ok((KeyTrustLevel::Unverified, None));
        }

        // Check for key custodian trust shortcut
        for (sender_hash, key_bytes) in &collector.responses {
            if self.is_trusted_authority_internal(sender_hash, channel_id, key_custodians) {
                let mut key = [0u8; 32];
                key.copy_from_slice(key_bytes);
                self.archive_keys
                    .insert(channel_id, (ChannelKey { key }, KeyTrustLevel::Verified));
                return Ok((KeyTrustLevel::Verified, Some(key)));
            }
        }

        // Compute client-side threshold
        let required_threshold = compute_consensus_threshold(collector.observed_members);

        // Check if all responses agree
        let mut key_groups: HashMap<Vec<u8>, Vec<String>> = HashMap::new();
        for (sender, key_bytes) in &collector.responses {
            key_groups
                .entry(key_bytes.clone())
                .or_default()
                .push(sender.clone());
        }

        if key_groups.len() == 1 {
            // All agree
            let (key_bytes, senders) = key_groups.into_iter().next().unwrap();
            let mut key = [0u8; 32];
            key.copy_from_slice(&key_bytes);

            let trust = if senders.len() as u32 >= required_threshold {
                KeyTrustLevel::Verified
            } else {
                KeyTrustLevel::Unverified
            };

            self.archive_keys
                .insert(channel_id, (ChannelKey { key }, trust));
            Ok((trust, Some(key)))
        } else {
            // Disagreement - check if any custodian key is present
            for (key_bytes, senders) in &key_groups {
                for sender in senders {
                    if self.is_trusted_authority_internal(sender, channel_id, key_custodians) {
                        let mut key = [0u8; 32];
                        key.copy_from_slice(key_bytes);
                        self.archive_keys.insert(
                            channel_id,
                            (ChannelKey { key }, KeyTrustLevel::Verified),
                        );
                        return Ok((KeyTrustLevel::Verified, Some(key)));
                    }
                }
            }

            // No custodian resolution - mark disputed
            // Accept the majority key tentatively
            let (majority_key, _) = key_groups
                .iter()
                .max_by_key(|(_, senders)| senders.len())
                .unwrap();
            let mut key = [0u8; 32];
            key.copy_from_slice(majority_key);
            self.archive_keys
                .insert(channel_id, (ChannelKey { key }, KeyTrustLevel::Disputed));
            Ok((KeyTrustLevel::Disputed, Some(key)))
        }
    }

    // ---- Epoch fork resolution --------------------------------------

    /// Resolve epoch fork candidates for a (channel, epoch) pair.
    ///
    /// Applies the deterministic tie-breaker: the candidate from the
    /// sender with the lexicographically smallest `cert_hash` wins.
    pub fn resolve_epoch_fork(
        &mut self,
        channel_id: u32,
        epoch: u32,
    ) -> Result<Option<String>> {
        let candidates = self
            .pending_epoch_candidates
            .remove(&(channel_id, epoch))
            .unwrap_or_default();

        if candidates.is_empty() {
            return Ok(None);
        }

        // Verify parent_fingerprint chain for each candidate
        let current_epoch_fp = self.current_epoch_fingerprint(channel_id);
        let valid_candidates: Vec<_> = candidates
            .into_iter()
            .filter(|c| {
                if let Some(fp) = current_epoch_fp {
                    c.parent_fingerprint == fp
                } else {
                    true // first epoch, no chain to verify
                }
            })
            .collect();

        if valid_candidates.is_empty() {
            return Err(Error::InvalidState(
                "no valid epoch candidates (parent_fingerprint mismatch)".into(),
            ));
        }

        // Deterministic tie-breaker: lowest cert_hash wins
        let winner = valid_candidates
            .iter()
            .min_by(|a, b| a.sender_hash.to_lowercase().cmp(&b.sender_hash.to_lowercase()))
            .unwrap();

        let winner_hash = winner.sender_hash.clone();
        let winner_key = winner.epoch_key.clone();

        self.epoch_keys
            .entry(channel_id)
            .or_default()
            .insert(epoch, (winner_key, KeyTrustLevel::Unverified));

        Ok(Some(winner_hash))
    }

    fn current_epoch_fingerprint(&self, channel_id: u32) -> Option<[u8; 8]> {
        self.epoch_keys
            .get(&channel_id)
            .and_then(|epochs| epochs.values().next_back())
            .map(|(key, _)| key.fingerprint())
    }

    // ---- Key distribution -------------------------------------------

    /// Generate a key-exchange payload for distributing a key to a new member.
    #[allow(clippy::too_many_arguments)]
    pub fn distribute_key(
        &self,
        channel_id: u32,
        mode: PersistenceMode,
        epoch: u32,
        recipient_hash: &str,
        recipient_public: &X25519PublicKey,
        request_id: Option<&str>,
        timestamp: u64,
    ) -> Result<PchatKeyExchange> {
        let key_bytes = match mode {
            PersistenceMode::PostJoin => {
                let epochs = self
                    .epoch_keys
                    .get(&channel_id)
                    .ok_or_else(|| Error::InvalidState("no epoch keys".into()))?;
                let (epoch_key, _) = epochs
                    .get(&epoch)
                    .ok_or_else(|| Error::InvalidState(format!("no key for epoch {epoch}")))?;
                epoch_key.key
            }
            PersistenceMode::FullArchive => {
                let (channel_key, _) = self
                    .archive_keys
                    .get(&channel_id)
                    .ok_or_else(|| Error::InvalidState("no archive key".into()))?;
                channel_key.key
            }
            _ => {
                return Err(Error::InvalidState(format!(
                    "cannot distribute key for mode {mode:?}"
                )));
            }
        };

        // Encrypt the key to the recipient's X25519 public key via DH
        let shared_secret = self.identity.dh_agree(recipient_public);
        let wrap_key = self
            .deriver
            .derive(&shared_secret, encryption::HKDF_SALT_IDENTITY, b"key-wrap")?;
        let encrypted_key = self.encryptor.encrypt(&wrap_key, &key_bytes, &[])?;

        // Compute fingerprints
        let efp = epoch_fingerprint(&key_bytes);
        let parent_fp = if mode == PersistenceMode::PostJoin {
            let prev_epoch = epoch.checked_sub(1);
            prev_epoch.and_then(|pe| {
                self.epoch_keys
                    .get(&channel_id)
                    .and_then(|epochs| epochs.get(&pe))
                    .map(|(k, _)| k.fingerprint().to_vec())
            })
        } else {
            None
        };

        // Build and sign
        let signed_data = build_key_exchange_signed_data(
            ALGORITHM_VERSION,
            channel_id,
            &mode,
            epoch,
            &encrypted_key,
            recipient_hash,
            request_id,
            timestamp,
        );
        let signature = self.identity.sign(&signed_data);

        Ok(PchatKeyExchange {
            channel_id,
            mode: mode.as_wire_str().to_string(),
            epoch,
            encrypted_key,
            sender_hash: String::new(), // caller fills in cert_hash
            recipient_hash: recipient_hash.to_string(),
            request_id: request_id.map(String::from),
            timestamp,
            algorithm_version: ALGORITHM_VERSION,
            signature: signature.to_bytes().to_vec(),
            parent_fingerprint: parent_fp,
            epoch_fingerprint: efp.to_vec(),
            countersignature: None,
            countersigner_hash: None,
        })
    }

    // ---- Key request handling ---------------------------------------

    /// Handle an incoming key request. Returns a key-exchange payload
    /// if we hold the key and have not exceeded the batch limit.
    pub fn handle_key_request(
        &mut self,
        request: &PchatKeyRequest,
        our_cert_hash: &str,
    ) -> Result<Option<PchatKeyExchange>> {
        if self.requests_processed >= self.max_requests_per_connection {
            return Ok(None);
        }

        if request.requester_public.len() != 32 {
            return Err(Error::InvalidState("invalid requester public key length".into()));
        }

        let mode = PersistenceMode::from_wire_str(&request.mode);
        let channel_id = request.channel_id;

        // Check if we hold the key for this channel
        let has_key = match mode {
            PersistenceMode::PostJoin => self.epoch_keys.contains_key(&channel_id),
            PersistenceMode::FullArchive => self.archive_keys.contains_key(&channel_id),
            _ => false,
        };

        if !has_key {
            return Ok(None);
        }

        let epoch = match mode {
            PersistenceMode::PostJoin => self
                .epoch_keys
                .get(&channel_id)
                .and_then(|epochs| epochs.keys().next_back().copied())
                .unwrap_or(0),
            _ => 0,
        };

        let mut requester_key_bytes = [0u8; 32];
        requester_key_bytes.copy_from_slice(&request.requester_public);
        let recipient_public = X25519PublicKey::from(requester_key_bytes);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let mut exchange = self.distribute_key(
            channel_id,
            mode,
            epoch,
            &request.requester_hash,
            &recipient_public,
            Some(&request.request_id),
            now,
        )?;
        exchange.sender_hash = our_cert_hash.to_string();

        self.requests_processed += 1;
        Ok(Some(exchange))
    }

    // ---- Countersignature verification ------------------------------

    /// Verify an epoch countersignature (standalone or inline).
    #[allow(clippy::too_many_arguments)]
    pub fn verify_countersignature(
        &mut self,
        channel_id: u32,
        epoch: u32,
        epoch_fp: &[u8],
        parent_fp: &[u8],
        signer_hash: &str,
        distributor_hash: &str,
        timestamp: u64,
        countersignature: &[u8],
        key_custodians: &[String],
    ) -> Result<KeyTrustLevel> {
        self.verify_countersignature_internal(
            channel_id,
            epoch,
            epoch_fp,
            parent_fp,
            signer_hash,
            distributor_hash,
            timestamp,
            countersignature,
        )?;

        // Verify signer is a trusted authority
        if !self.is_trusted_authority_internal(signer_hash, channel_id, key_custodians) {
            return Err(Error::InvalidState(
                "countersigner is not a trusted authority".into(),
            ));
        }

        // Promote epoch key to Verified
        if let Some(epochs) = self.epoch_keys.get_mut(&channel_id) {
            if let Some((_key, trust)) = epochs.get_mut(&epoch) {
                *trust = KeyTrustLevel::Verified;
            }
        }

        Ok(KeyTrustLevel::Verified)
    }

    #[allow(clippy::too_many_arguments)]
    fn verify_countersignature_internal(
        &self,
        channel_id: u32,
        epoch: u32,
        epoch_fp: &[u8],
        parent_fp: &[u8],
        signer_hash: &str,
        distributor_hash: &str,
        timestamp: u64,
        countersignature: &[u8],
    ) -> Result<()> {
        // Timestamp freshness
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        if now > timestamp + COUNTERSIG_FRESHNESS_MS {
            return Err(Error::InvalidState(
                "countersignature timestamp too old".into(),
            ));
        }

        // Verify Ed25519 signature
        let peer = self.peer_keys.get(signer_hash).ok_or_else(|| {
            Error::InvalidState(format!("unknown countersigner: {signer_hash}"))
        })?;

        let data =
            build_countersig_data(channel_id, epoch, epoch_fp, parent_fp, timestamp, distributor_hash);

        let sig = ed25519_dalek::Signature::from_slice(countersignature)
            .map_err(|e| Error::InvalidState(format!("invalid countersig bytes: {e}")))?;

        peer.signing_public
            .verify(&data, &sig)
            .map_err(|e| Error::InvalidState(format!("countersignature invalid: {e}")))
    }

    // ---- Trust authority checks -------------------------------------

    /// Check if a sender is a trusted authority for a channel.
    ///
    /// Returns true only when all conditions from section 5.7 are met:
    /// 1. Sender appears in `key_custodians` or is the channel originator.
    /// 2. Sender appears in the TOFU-pinned list.
    /// 3. The pinned list has been confirmed by the user.
    pub fn is_trusted_authority(
        &self,
        sender_hash: &str,
        channel_id: u32,
        key_custodians: &[String],
    ) -> bool {
        self.is_trusted_authority_internal(sender_hash, channel_id, key_custodians)
    }

    fn is_trusted_authority_internal(
        &self,
        sender_hash: &str,
        channel_id: u32,
        key_custodians: &[String],
    ) -> bool {
        // Check 1: sender in custodians or is channel originator
        let in_server_list = key_custodians.iter().any(|h| h == sender_hash);
        let is_originator = self
            .channel_originators
            .get(&channel_id)
            .is_some_and(|h| h == sender_hash);

        if !in_server_list && !is_originator {
            return false;
        }

        // Check 2 & 3: sender in confirmed pinned list
        if let Some(pin_state) = self.pinned_custodians.get(&channel_id) {
            if !pin_state.confirmed {
                return false;
            }
            pin_state.pinned.iter().any(|h| h == sender_hash) || is_originator
        } else {
            // No pinned state yet - only originator is trusted
            is_originator
        }
    }

    // ---- Custodian TOFU management ----------------------------------

    /// Update the pinned custodian list from a `ChannelState` update.
    ///
    /// Returns `true` if the list changed and needs user acceptance.
    pub fn update_custodian_pin(
        &mut self,
        channel_id: u32,
        new_custodians: Vec<String>,
    ) -> bool {
        if let Some(pin_state) = self.pinned_custodians.get_mut(&channel_id) {
            if pin_state.pinned == new_custodians {
                return false; // no change
            }
            pin_state.pending_update = Some(new_custodians);
            true
        } else {
            // First observation
            self.pinned_custodians
                .insert(channel_id, CustodianPinState::first_observation(new_custodians));
            // Needs confirmation if list is non-empty
            self.pinned_custodians.get(&channel_id).is_some_and(|s| !s.confirmed)
        }
    }

    /// Accept a pending custodian list update (user clicked "Accept").
    pub fn accept_custodian_update(&mut self, channel_id: u32) {
        if let Some(pin_state) = self.pinned_custodians.get_mut(&channel_id) {
            if let Some(new_list) = pin_state.pending_update.take() {
                pin_state.pinned = new_list;
            }
            pin_state.confirmed = true;
        }
    }

    /// Confirm the initial custodian list (user clicked "Confirm" on first join).
    pub fn confirm_custodian_list(&mut self, channel_id: u32) {
        if let Some(pin_state) = self.pinned_custodians.get_mut(&channel_id) {
            pin_state.confirmed = true;
        }
    }

    /// Get the current custodian pin state for a channel.
    pub fn get_custodian_pin(&self, channel_id: u32) -> Option<&CustodianPinState> {
        self.pinned_custodians.get(&channel_id)
    }

    // ---- Key trial decryption (supplementary check) -----------------

    /// Attempt to verify a key by decrypting recent messages.
    ///
    /// Returns true if decryption succeeds for messages from 2+ distinct
    /// senders. This is a diagnostic signal only and does NOT promote
    /// trust level.
    pub fn check_key_by_decryption(
        &self,
        channel_id: u32,
        mode: PersistenceMode,
        messages: &[StoredMessage],
    ) -> bool {
        let mut successful_senders = std::collections::HashSet::new();

        for msg in messages {
            if !msg.encrypted {
                continue;
            }
            let payload = EncryptedPayload {
                ciphertext: msg.body.as_bytes().to_vec(),
                epoch: msg.epoch,
                chain_index: msg.chain_index,
                epoch_fingerprint: [0; 8], // not checked here
            };
            if self
                .decrypt(mode, channel_id, &msg.message_id, msg.timestamp, &payload)
                .is_ok()
            {
                successful_senders.insert(&msg.sender_hash);
            }
        }

        successful_senders.len() >= 2
    }

    // ---- Dispute resolution -----------------------------------------

    /// Resolve a dispute by manually selecting a trusted peer's key.
    pub fn resolve_dispute(
        &mut self,
        channel_id: u32,
        mode: PersistenceMode,
        _trusted_sender_hash: &str,
    ) -> Result<()> {
        match mode {
            PersistenceMode::FullArchive => {
                if let Some((_key, trust)) = self.archive_keys.get_mut(&channel_id) {
                    *trust = KeyTrustLevel::ManuallyVerified;
                }
                Ok(())
            }
            PersistenceMode::PostJoin => {
                if let Some(epochs) = self.epoch_keys.get_mut(&channel_id) {
                    if let Some((_key, trust)) = epochs.values_mut().next_back() {
                        *trust = KeyTrustLevel::ManuallyVerified;
                    }
                }
                Ok(())
            }
            _ => Err(Error::InvalidState(format!(
                "cannot resolve dispute for mode {mode:?}"
            ))),
        }
    }

    // ---- Trust level query ------------------------------------------

    /// Get the trust level for a channel's current key.
    pub fn trust_level(
        &self,
        channel_id: u32,
        mode: PersistenceMode,
    ) -> Option<KeyTrustLevel> {
        match mode {
            PersistenceMode::PostJoin => self
                .epoch_keys
                .get(&channel_id)
                .and_then(|epochs| epochs.values().next_back())
                .map(|(_, trust)| *trust),
            PersistenceMode::FullArchive => {
                self.archive_keys.get(&channel_id).map(|(_, trust)| *trust)
            }
            _ => None,
        }
    }

    // ---- Key announcement generation --------------------------------

    /// Build a `fancy-pchat-key-announce` payload for our identity.
    pub fn build_key_announce(&self, cert_hash: &str, timestamp: u64) -> PchatKeyAnnounce {
        let identity_public = self.dh_public_bytes().to_vec();
        let signing_public = self.signing_public_bytes().to_vec();

        let signed_data = encryption::build_key_announce_signed_data(
            ALGORITHM_VERSION,
            cert_hash,
            timestamp,
            &identity_public,
            &signing_public,
        );
        let signature = self.identity.sign(&signed_data);

        PchatKeyAnnounce {
            algorithm_version: ALGORITHM_VERSION,
            identity_public,
            signing_public,
            cert_hash: cert_hash.to_string(),
            timestamp,
            signature: signature.to_bytes().to_vec(),
            tls_signature: Vec::new(), // filled in by the caller (requires TLS private key)
        }
    }

    // ---- Epoch key management (direct insertion) --------------------

    /// Store an epoch key directly (e.g. when this client generates a new epoch).
    pub fn store_epoch_key(
        &mut self,
        channel_id: u32,
        epoch: u32,
        key: [u8; 32],
        trust: KeyTrustLevel,
    ) {
        self.epoch_keys
            .entry(channel_id)
            .or_default()
            .insert(epoch, (EpochKey::new(key), trust));
    }

    /// Store an archive key directly.
    pub fn store_archive_key(
        &mut self,
        channel_id: u32,
        key: [u8; 32],
        trust: KeyTrustLevel,
    ) {
        self.archive_keys
            .insert(channel_id, (ChannelKey { key }, trust));
    }

    /// Record a channel key originator.
    pub fn set_channel_originator(&mut self, channel_id: u32, cert_hash: String) {
        self.channel_originators.insert(channel_id, cert_hash);
    }
}

// ---- Helpers --------------------------------------------------------

/// Compute the consensus threshold from observed member count.
///
/// `required_threshold = clamp(floor(observed_members / 2), 1, 5)`
fn compute_consensus_threshold(observed_members: u32) -> u32 {
    (observed_members / 2).clamp(1, 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_seed() -> [u8; 32] {
        [0xAA; 32]
    }

    fn make_key_manager() -> KeyManager {
        let identity = SeedIdentity::from_seed(&test_seed()).unwrap();
        KeyManager::new(Box::new(identity))
    }

    #[test]
    fn seed_identity_deterministic() {
        let seed = test_seed();
        let id1 = SeedIdentity::from_seed(&seed).unwrap();
        let id2 = SeedIdentity::from_seed(&seed).unwrap();
        assert_eq!(id1.dh_public_key().as_bytes(), id2.dh_public_key().as_bytes());
        assert_eq!(id1.signing_public_key().to_bytes(), id2.signing_public_key().to_bytes());
    }

    #[test]
    fn different_seeds_different_keys() {
        let id1 = SeedIdentity::from_seed(&[0x01; 32]).unwrap();
        let id2 = SeedIdentity::from_seed(&[0x02; 32]).unwrap();
        assert_ne!(id1.dh_public_key().as_bytes(), id2.dh_public_key().as_bytes());
    }

    #[test]
    fn encrypt_decrypt_full_archive() {
        let mut km = make_key_manager();
        let key = [0x42u8; 32];
        km.store_archive_key(1, key, KeyTrustLevel::Verified);

        let msg_id = uuid::Uuid::new_v4().to_string();
        let plaintext = b"Hello, world!";
        let payload = km
            .encrypt(PersistenceMode::FullArchive, 1, &msg_id, 1000, plaintext)
            .unwrap();

        let decrypted = km
            .decrypt(PersistenceMode::FullArchive, 1, &msg_id, 1000, &payload)
            .unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_decrypt_post_join() {
        let mut km = make_key_manager();
        let key = [0x55u8; 32];
        km.store_epoch_key(1, 0, key, KeyTrustLevel::Verified);

        let msg_id = uuid::Uuid::new_v4().to_string();
        let plaintext = b"Epoch message";
        let payload = km
            .encrypt(PersistenceMode::PostJoin, 1, &msg_id, 2000, plaintext)
            .unwrap();

        assert_eq!(payload.epoch, Some(0));
        assert_eq!(payload.chain_index, Some(0));

        let decrypted = km
            .decrypt(PersistenceMode::PostJoin, 1, &msg_id, 2000, &payload)
            .unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn chain_ratchet_advances() {
        let mut km = make_key_manager();
        km.store_epoch_key(1, 0, [0x55; 32], KeyTrustLevel::Verified);

        let id1 = uuid::Uuid::new_v4().to_string();
        let p1 = km
            .encrypt(PersistenceMode::PostJoin, 1, &id1, 100, b"msg1")
            .unwrap();
        assert_eq!(p1.chain_index, Some(0));

        let id2 = uuid::Uuid::new_v4().to_string();
        let p2 = km
            .encrypt(PersistenceMode::PostJoin, 1, &id2, 200, b"msg2")
            .unwrap();
        assert_eq!(p2.chain_index, Some(1));

        // Both should decrypt correctly
        assert_eq!(
            km.decrypt(PersistenceMode::PostJoin, 1, &id1, 100, &p1)
                .unwrap(),
            b"msg1"
        );
        assert_eq!(
            km.decrypt(PersistenceMode::PostJoin, 1, &id2, 200, &p2)
                .unwrap(),
            b"msg2"
        );
    }

    #[test]
    fn trust_level_query() {
        let mut km = make_key_manager();
        assert!(km.trust_level(1, PersistenceMode::PostJoin).is_none());

        km.store_epoch_key(1, 0, [0; 32], KeyTrustLevel::Unverified);
        assert_eq!(
            km.trust_level(1, PersistenceMode::PostJoin),
            Some(KeyTrustLevel::Unverified)
        );

        km.store_archive_key(2, [0; 32], KeyTrustLevel::Verified);
        assert_eq!(
            km.trust_level(2, PersistenceMode::FullArchive),
            Some(KeyTrustLevel::Verified)
        );
    }

    #[test]
    fn consensus_threshold_computation() {
        assert_eq!(compute_consensus_threshold(0), 1);
        assert_eq!(compute_consensus_threshold(1), 1);
        assert_eq!(compute_consensus_threshold(2), 1);
        assert_eq!(compute_consensus_threshold(3), 1);
        assert_eq!(compute_consensus_threshold(4), 2);
        assert_eq!(compute_consensus_threshold(10), 5);
        assert_eq!(compute_consensus_threshold(100), 5);
    }

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
    fn trusted_authority_requires_confirmation() {
        let mut km = make_key_manager();
        let custodians = vec!["alice_hash".to_string()];

        // Pin without confirmation
        km.update_custodian_pin(1, custodians.clone());
        assert!(!km.is_trusted_authority("alice_hash", 1, &custodians));

        // Confirm
        km.confirm_custodian_list(1);
        assert!(km.is_trusted_authority("alice_hash", 1, &custodians));
    }

    #[test]
    fn channel_originator_is_trusted() {
        let mut km = make_key_manager();
        km.set_channel_originator(1, "bob_hash".into());
        // Originator trusted even without custodian list
        assert!(km.is_trusted_authority("bob_hash", 1, &[]));
    }

    #[test]
    fn key_announce_roundtrip() {
        let km = make_key_manager();
        let announce = km.build_key_announce("test_cert", 12345);
        assert_eq!(announce.algorithm_version, ALGORITHM_VERSION);
        assert_eq!(announce.cert_hash, "test_cert");
        assert_eq!(announce.identity_public.len(), 32);
        assert_eq!(announce.signing_public.len(), 32);

        // Another km can verify and record the peer
        let mut km2 = make_key_manager();
        let result = km2.record_peer_key(&announce);
        assert!(result.is_ok());
        assert!(km2.get_peer("test_cert").is_some());
    }

    #[test]
    fn anti_rollback_rejects_stale_announce() {
        let km = make_key_manager();
        let announce1 = km.build_key_announce("peer1", 100);
        let announce2 = km.build_key_announce("peer1", 50); // older timestamp

        let mut km2 = make_key_manager();
        assert!(km2.record_peer_key(&announce1).unwrap());
        // Stale announcement should be silently discarded
        assert!(!km2.record_peer_key(&announce2).unwrap());
    }

    #[test]
    fn epoch_fork_resolution_picks_lowest_hash() {
        let mut km = make_key_manager();

        let candidates = vec![
            EpochCandidate {
                sender_hash: "zzz_hash".into(),
                epoch_key: EpochKey::new([0x01; 32]),
                parent_fingerprint: [0; 8],
                epoch_fingerprint: epoch_fingerprint(&[0x01; 32]),
                received_at: Instant::now(),
            },
            EpochCandidate {
                sender_hash: "aaa_hash".into(),
                epoch_key: EpochKey::new([0x02; 32]),
                parent_fingerprint: [0; 8],
                epoch_fingerprint: epoch_fingerprint(&[0x02; 32]),
                received_at: Instant::now(),
            },
        ];

        km.pending_epoch_candidates.insert((1, 0), candidates);
        let winner = km.resolve_epoch_fork(1, 0).unwrap();
        assert_eq!(winner, Some("aaa_hash".to_string()));
    }

    #[test]
    fn channel_key_fingerprint_deterministic() {
        let key = [0xBB; 32];
        let fp1 = channel_key_fingerprint(&key, 1, PersistenceMode::FullArchive);
        let fp2 = channel_key_fingerprint(&key, 1, PersistenceMode::FullArchive);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn channel_key_fingerprint_differs_by_channel() {
        let key = [0xBB; 32];
        let fp1 = channel_key_fingerprint(&key, 1, PersistenceMode::FullArchive);
        let fp2 = channel_key_fingerprint(&key, 2, PersistenceMode::FullArchive);
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn channel_key_fingerprint_differs_by_mode() {
        let key = [0xBB; 32];
        let fp1 = channel_key_fingerprint(&key, 1, PersistenceMode::PostJoin);
        let fp2 = channel_key_fingerprint(&key, 1, PersistenceMode::FullArchive);
        assert_ne!(fp1, fp2);
    }
}
