//! Pluggable E2EE protocol abstraction.
//!
//! The [`E2EEProtocol`] trait defines the contract for a complete
//! end-to-end encryption protocol. Each implementation bundles
//! identity management, key exchange, message encryption, trust
//! evaluation, and wire format handling into a single replaceable unit.
//!
//! # Built-in protocols
//!
//! | Version | Module | Description |
//! |---------|--------|-------------|
//! | 1 | [`fancy_v1`] | XChaCha20-Poly1305 + HKDF-SHA256, X25519/Ed25519 identity |
//!
//! # Adding a new protocol
//!
//! 1. Create a new sub-module (e.g. `signal_v2`).
//! 2. Implement [`E2EEProtocol`] for a new struct.
//! 3. Update `KeyManager` to accept a `Box<dyn E2EEProtocol>`.

use crate::error::Result;
use crate::persistent::wire;
use crate::persistent::PchatProtocol;

pub mod fancy_v1;

// ---- Parameter structs ----------------------------------------------

/// Parameters for [`E2EEProtocol::distribute_key`].
#[derive(Debug, Clone)]
pub struct DistributeKeyParams<'a> {
    /// Target channel ID.
    pub channel_id: u32,
    /// Persistence protocol of the channel.
    pub mode: PchatProtocol,
    /// Epoch number for this key.
    pub epoch: u32,
    /// Raw key material (32 bytes).
    pub key_bytes: &'a [u8],
    /// TLS cert hash of the recipient.
    pub recipient_hash: &'a str,
    /// Recipient's DH public key bytes.
    pub recipient_dh_public: &'a [u8],
    /// Optional request ID this distribution responds to.
    pub request_id: Option<&'a str>,
    /// Timestamp (Unix epoch ms).
    pub timestamp: u64,
    /// Parent epoch fingerprint for chain verification.
    pub parent_fingerprint: Option<[u8; 8]>,
}

/// Parameters for [`E2EEProtocol::verify_countersignature`].
#[derive(Debug, Clone)]
pub struct VerifyCountersigParams<'a> {
    /// Channel ID the countersignature covers.
    pub channel_id: u32,
    /// Epoch number.
    pub epoch: u32,
    /// Epoch key fingerprint.
    pub epoch_fingerprint: &'a [u8; 8],
    /// Parent epoch fingerprint.
    pub parent_fingerprint: &'a [u8; 8],
    /// Signer's Ed25519 (or equivalent) public key bytes.
    pub signer_signing_public: &'a [u8],
    /// TLS cert hash of the key distributor.
    pub distributor_hash: &'a str,
    /// Timestamp (Unix epoch ms).
    pub timestamp: u64,
    /// The countersignature bytes to verify.
    pub countersignature: &'a [u8],
}

// ---- Protocol trait -------------------------------------------------

/// A complete E2EE protocol implementation.
///
/// Bundles all cryptographic operations needed for persistent chat:
/// identity, key exchange, message encryption, trust model.
///
/// Implementations must be `Send + Sync` so they can be shared across
/// async tasks.
pub trait E2EEProtocol: Send + Sync {
    /// Protocol version byte embedded in key announcements and
    /// encrypted payloads. Must be unique across all registered protocols.
    fn version(&self) -> u8;

    /// Human-readable name for logging and diagnostics.
    fn name(&self) -> &'static str;

    // ---- Identity ---------------------------------------------------

    /// Our X25519 (or equivalent) DH public key bytes.
    fn dh_public_bytes(&self) -> Vec<u8>;

    /// Our Ed25519 (or equivalent) signing public key bytes.
    fn signing_public_bytes(&self) -> Vec<u8>;

    // ---- Key announcement -------------------------------------------

    /// Build a key announcement message advertising our public keys.
    fn build_key_announce(
        &self,
        cert_hash: &str,
        timestamp: u64,
    ) -> Result<wire::PchatKeyAnnounce>;

    /// Validate and record a peer's key announcement.
    ///
    /// Returns the peer's cert hash on success, or an error if
    /// validation fails (bad signature, anti-rollback, etc.).
    fn record_peer_key_announce(
        &mut self,
        announce: &wire::PchatKeyAnnounce,
    ) -> Result<String>;

    // ---- Key exchange -----------------------------------------------

    /// Build a key exchange message to distribute a key to a recipient.
    ///
    /// The caller provides the raw key material and recipient info.
    fn distribute_key(
        &self,
        params: &DistributeKeyParams<'_>,
    ) -> Result<wire::PchatKeyExchange>;

    /// Validate and decrypt a received key exchange message.
    ///
    /// Returns the decrypted key bytes, epoch fingerprint from the
    /// exchange, and the optional parent fingerprint on success.
    fn receive_key_exchange(
        &self,
        exchange: &wire::PchatKeyExchange,
        sender_dh_public: &[u8],
        sender_signing_public: &[u8],
        request_timestamp: Option<u64>,
    ) -> Result<ReceivedKey>;

    // ---- Message encryption -----------------------------------------

    /// Encrypt a plaintext message for a channel.
    fn encrypt_message(
        &self,
        key: &[u8; 32],
        channel_id: u32,
        message_id: &[u8; 16],
        timestamp: u64,
    ) -> Result<EncryptionContext>;

    /// Decrypt an encrypted message payload.
    fn decrypt_message(
        &self,
        key: &[u8; 32],
        channel_id: u32,
        message_id: &[u8; 16],
        timestamp: u64,
        payload: &[u8],
    ) -> Result<Vec<u8>>;

    // ---- Chain ratchet (PostJoin only) -------------------------------

    /// Derive the next chain key from the current one.
    fn derive_chain_key(&self, current: &[u8; 32]) -> Result<[u8; 32]>;

    /// Derive a per-message encryption key from a chain key.
    fn derive_message_key(&self, chain_key: &[u8; 32]) -> Result<[u8; 32]>;

    /// Derive the message key at a specific chain index.
    fn derive_key_at_index(&self, epoch_key: &[u8; 32], target_index: u32) -> Result<[u8; 32]>;

    // ---- Archive key derivation (FullArchive only) -------------------

    /// Deterministically derive an archive key from a seed and channel ID.
    fn derive_archive_key(&self, seed: &[u8; 32], channel_id: u32) -> [u8; 32];

    // ---- Fingerprinting ---------------------------------------------

    /// Compute a short fingerprint for a key (for verification).
    fn key_fingerprint(&self, key: &[u8]) -> [u8; 8];

    // ---- Trust model ------------------------------------------------

    /// Evaluate consensus on a received key.
    ///
    /// Given a set of responses and channel context, determine the
    /// trust level for the key.
    fn compute_consensus_threshold(&self, observed_members: u32) -> u32;

    /// Verify a key custodian countersignature.
    fn verify_countersignature(
        &self,
        params: &VerifyCountersigParams<'_>,
    ) -> Result<bool>;

    /// Build a countersignature for an epoch transition.
    fn build_countersignature(
        &self,
        channel_id: u32,
        epoch: u32,
        epoch_fingerprint: &[u8; 8],
        parent_fingerprint: &[u8; 8],
        distributor_hash: &str,
        timestamp: u64,
    ) -> Result<Vec<u8>>;

    // ---- Challenge-response (key possession proof) -------------------

    /// Compute HMAC proof of archive key possession.
    fn compute_challenge_proof(
        &self,
        archive_key: &[u8; 32],
        challenge: &[u8],
    ) -> Result<[u8; 32]>;
}

// ---- Protocol output types ------------------------------------------

/// Result of receiving a key exchange message.
#[derive(Debug, Clone)]
pub struct ReceivedKey {
    /// The decrypted raw key bytes (32 bytes).
    pub key_bytes: [u8; 32],
    /// Epoch fingerprint from the exchange.
    pub epoch_fingerprint: [u8; 8],
    /// Parent fingerprint (`PostJoin` chain verification).
    pub parent_fingerprint: Option<[u8; 8]>,
}

/// Context needed to finalize message encryption.
///
/// The protocol builds the AAD and derives the encryption key,
/// then the caller uses these to encrypt the actual plaintext
/// through the protocol's encryptor.
#[derive(Debug, Clone)]
pub struct EncryptionContext {
    /// The derived per-message encryption key.
    pub encryption_key: [u8; 32],
    /// Associated authenticated data for AEAD.
    pub aad: Vec<u8>,
}

// ---- Supported versions ---------------------------------------------

/// List all supported protocol version numbers.
#[must_use]
pub fn supported_versions() -> &'static [u8] {
    &[1]
}
