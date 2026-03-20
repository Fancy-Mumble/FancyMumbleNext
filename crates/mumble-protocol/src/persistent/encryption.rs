//! XChaCha20-Poly1305 encryption with HKDF-SHA256 key derivation.
//!
//! Implements the encryption envelope described in design doc section 4:
//! `[Version(1B)] [Nonce(24B)] [AEAD Ciphertext + Tag(16B)]`
//!
//! Provides traits so alternative ciphers can be plugged in.

use crate::error::{Error, Result};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::Rng;
use sha2::Sha256;

/// Current encryption version byte.
pub const ENCRYPTION_VERSION: u8 = 0x01;

/// HKDF info string for deriving chain keys.
pub const HKDF_INFO_CHAIN: &[u8] = b"fancy-pchat-chain-v1";
/// HKDF info string for deriving per-message keys.
pub const HKDF_INFO_MSG: &[u8] = b"fancy-pchat-msg-v1";
/// HKDF salt for identity key derivation.
pub const HKDF_SALT_IDENTITY: &[u8] = b"fancy-pchat-v1";
/// HKDF info for X25519 key derivation from seed.
pub const HKDF_INFO_X25519: &[u8] = b"x25519";
/// HKDF info for Ed25519 key derivation from seed.
pub const HKDF_INFO_ED25519: &[u8] = b"ed25519";
/// HKDF salt for deterministic archive key derivation from seed + `channel_id`.
pub const HKDF_SALT_ARCHIVE_KEY: &[u8] = b"fancy-pchat-archive-key-v1";

/// Derive a deterministic archive key from the identity seed and channel ID.
///
/// Convenience wrapper around [`HkdfArchiveKeyDeriver`].
pub fn derive_archive_key(seed: &[u8; 32], channel_id: u32) -> [u8; 32] {
    HkdfArchiveKeyDeriver.derive_archive_key(seed, channel_id)
}

/// Block size for ciphertext padding (bytes).
const PADDING_BLOCK: usize = 256;
/// XChaCha20-Poly1305 nonce length.
const NONCE_LEN: usize = 24;
/// XChaCha20-Poly1305 key length.
const KEY_LEN: usize = 32;

// ---- Trait: Encryptor -----------------------------------------------

/// Trait abstracting symmetric encryption and decryption.
///
/// Enables testing with mock ciphers and future algorithm upgrades.
pub trait Encryptor: Send + Sync {
    /// Encrypt plaintext with the given key and associated data.
    ///
    /// Returns the encrypted payload including version byte, nonce,
    /// and AEAD ciphertext+tag.
    fn encrypt(&self, key: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>>;

    /// Decrypt a payload produced by [`encrypt`](Encryptor::encrypt).
    fn decrypt(&self, key: &[u8], payload: &[u8], aad: &[u8]) -> Result<Vec<u8>>;

    /// The version byte this encryptor produces and accepts.
    fn version(&self) -> u8;
}

// ---- Trait: KeyDeriver ----------------------------------------------

/// Trait abstracting key derivation (HKDF).
///
/// Separated so that key derivation logic can be tested independently.
pub trait KeyDeriver: Send + Sync {
    /// Derive a fixed-length key from input key material.
    fn derive(&self, ikm: &[u8], salt: &[u8], info: &[u8]) -> Result<[u8; KEY_LEN]>;
}

// ---- Trait: ChainRatchet --------------------------------------------

/// Trait abstracting the symmetric ratchet used to derive chain keys
/// and per-message keys from an epoch key.
///
/// Enables alternative ratchet strategies (e.g. skip-ahead, Double Ratchet).
pub trait ChainRatchet: Send + Sync {
    /// Derive the next chain key from the current one.
    fn derive_chain_key(&self, current: &[u8; KEY_LEN]) -> Result<[u8; KEY_LEN]>;

    /// Derive a per-message encryption key from a chain key.
    fn derive_message_key(&self, chain_key: &[u8; KEY_LEN]) -> Result<[u8; KEY_LEN]>;

    /// Derive the message key at a specific ratchet index by advancing
    /// from the epoch key. The default implementation ratchets forward
    /// sequentially.
    fn derive_key_at_index(
        &self,
        epoch_key: &[u8; KEY_LEN],
        target_index: u32,
    ) -> Result<[u8; KEY_LEN]> {
        let mut chain_key = *epoch_key;
        for _ in 0..target_index {
            chain_key = self.derive_chain_key(&chain_key)?;
        }
        self.derive_message_key(&chain_key)
    }
}

// ---- Trait: AadBuilder ----------------------------------------------

/// Trait abstracting the construction of AEAD Associated Authenticated Data.
pub trait AadBuilder: Send + Sync {
    /// Build AAD bytes from channel metadata.
    ///
    /// `AAD = channel_id(4B BE) || message_id(16B UUID) || timestamp(8B BE)`
    fn build_aad(&self, channel_id: u32, message_id: &[u8; 16], timestamp: u64) -> Vec<u8>;

    /// Parse a UUID string into 16 raw bytes.
    fn uuid_to_bytes(&self, uuid_str: &str) -> Result<[u8; 16]>;
}

// ---- Trait: SignedDataBuilder ----------------------------------------

/// Trait abstracting the byte-level layout of data that gets signed
/// in protocol messages.
pub trait SignedDataBuilder: Send + Sync {
    /// Build data for epoch countersignatures.
    fn build_countersig_data(
        &self,
        channel_id: u32,
        epoch: u32,
        epoch_fp: &[u8],
        parent_fp: &[u8],
        timestamp: u64,
        distributor_hash: &str,
    ) -> Vec<u8>;

    /// Build data signed in a key-exchange message.
    #[allow(clippy::too_many_arguments, reason = "protocol message construction requires all fields to be present")]
    fn build_key_exchange_signed_data(
        &self,
        algorithm_version: u8,
        channel_id: u32,
        mode: &crate::persistent::PersistenceMode,
        epoch: u32,
        encrypted_key: &[u8],
        recipient_hash: &str,
        request_id: Option<&str>,
        timestamp: u64,
    ) -> Vec<u8>;

    /// Build data signed in a key-announce message.
    fn build_key_announce_signed_data(
        &self,
        algorithm_version: u8,
        cert_hash: &str,
        timestamp: u64,
        identity_public: &[u8],
        signing_public: &[u8],
    ) -> Vec<u8>;
}

// ---- Trait: Fingerprinter -------------------------------------------

/// Trait abstracting epoch key fingerprint computation.
pub trait Fingerprinter: Send + Sync {
    /// Compute a short fingerprint of a key.
    fn epoch_fingerprint(&self, key: &[u8]) -> [u8; 8];
}

// ---- Trait: ArchiveKeyDeriver ----------------------------------------

/// Trait abstracting deterministic archive key derivation from
/// an identity seed and channel ID.
pub trait ArchiveKeyDeriver: Send + Sync {
    /// Derive a deterministic archive key.
    fn derive_archive_key(&self, seed: &[u8; 32], channel_id: u32) -> [u8; 32];
}

// ---- XChaCha20-Poly1305 implementation ------------------------------

/// Production encryptor using XChaCha20-Poly1305.
#[derive(Debug, Clone, Default)]
pub struct XChaChaEncryptor;

impl Encryptor for XChaChaEncryptor {
    fn encrypt(&self, key: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        if key.len() != KEY_LEN {
            return Err(Error::InvalidState(format!(
                "encryption key must be {KEY_LEN} bytes, got {}",
                key.len()
            )));
        }

        let padded = pad_plaintext(plaintext)?;

        let cipher =
            XChaCha20Poly1305::new_from_slice(key).map_err(|e| Error::Other(e.to_string()))?;

        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = XNonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(
                nonce,
                chacha20poly1305::aead::Payload {
                    msg: &padded,
                    aad,
                },
            )
            .map_err(|e| Error::Other(format!("encryption failed: {e}")))?;

        // Version(1) + Nonce(24) + Ciphertext+Tag
        let mut output = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
        output.push(ENCRYPTION_VERSION);
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    fn decrypt(&self, key: &[u8], payload: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        if key.len() != KEY_LEN {
            return Err(Error::InvalidState(format!(
                "decryption key must be {KEY_LEN} bytes, got {}",
                key.len()
            )));
        }

        let min_len = 1 + NONCE_LEN + 16; // version + nonce + tag
        if payload.len() < min_len {
            return Err(Error::InvalidState("payload too short".into()));
        }

        let version = payload[0];
        if version != ENCRYPTION_VERSION {
            return Err(Error::InvalidState(format!(
                "unsupported encryption version: {version:#04x}"
            )));
        }

        let nonce = XNonce::from_slice(&payload[1..1 + NONCE_LEN]);
        let ciphertext = &payload[1 + NONCE_LEN..];

        let cipher =
            XChaCha20Poly1305::new_from_slice(key).map_err(|e| Error::Other(e.to_string()))?;

        let padded = cipher
            .decrypt(
                nonce,
                chacha20poly1305::aead::Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|e| Error::Other(format!("decryption failed: {e}")))?;

        unpad_plaintext(&padded)
    }

    fn version(&self) -> u8 {
        ENCRYPTION_VERSION
    }
}

// ---- HKDF-SHA256 implementation -------------------------------------

/// Production key deriver using HKDF-SHA256.
#[derive(Debug, Clone, Default)]
pub struct HkdfSha256Deriver;

impl KeyDeriver for HkdfSha256Deriver {
    fn derive(&self, ikm: &[u8], salt: &[u8], info: &[u8]) -> Result<[u8; KEY_LEN]> {
        let hkdf = Hkdf::<Sha256>::new(Some(salt), ikm);
        let mut okm = [0u8; KEY_LEN];
        hkdf.expand(info, &mut okm)
            .map_err(|e| Error::Other(format!("HKDF expand failed: {e}")))?;
        Ok(okm)
    }
}

// ---- HkdfChainRatchet -----------------------------------------------

/// Chain ratchet using HKDF-SHA256 key derivation.
#[derive(Debug, Clone, Default)]
pub struct HkdfChainRatchet;

impl ChainRatchet for HkdfChainRatchet {
    fn derive_chain_key(&self, current: &[u8; KEY_LEN]) -> Result<[u8; KEY_LEN]> {
        HkdfSha256Deriver.derive(current, HKDF_SALT_IDENTITY, HKDF_INFO_CHAIN)
    }

    fn derive_message_key(&self, chain_key: &[u8; KEY_LEN]) -> Result<[u8; KEY_LEN]> {
        HkdfSha256Deriver.derive(chain_key, HKDF_SALT_IDENTITY, HKDF_INFO_MSG)
    }
}

// ---- StandardAadBuilder ---------------------------------------------

/// Standard AAD builder: `channel_id(4B) || message_id(16B) || timestamp(8B)`.
#[derive(Debug, Clone, Default)]
pub struct StandardAadBuilder;

impl AadBuilder for StandardAadBuilder {
    fn build_aad(&self, channel_id: u32, message_id: &[u8; 16], timestamp: u64) -> Vec<u8> {
        let mut aad = Vec::with_capacity(4 + 16 + 8);
        aad.extend_from_slice(&channel_id.to_be_bytes());
        aad.extend_from_slice(message_id);
        aad.extend_from_slice(&timestamp.to_be_bytes());
        aad
    }

    fn uuid_to_bytes(&self, uuid_str: &str) -> Result<[u8; 16]> {
        let id = uuid::Uuid::parse_str(uuid_str)
            .map_err(|e| Error::InvalidState(format!("invalid UUID: {e}")))?;
        Ok(*id.as_bytes())
    }
}

// ---- StandardSignedDataBuilder --------------------------------------

/// Standard byte-layout for signed protocol messages.
#[derive(Debug, Clone, Default)]
pub struct StandardSignedDataBuilder;

impl SignedDataBuilder for StandardSignedDataBuilder {
    fn build_countersig_data(
        &self,
        channel_id: u32,
        epoch: u32,
        epoch_fp: &[u8],
        parent_fp: &[u8],
        timestamp: u64,
        distributor_hash: &str,
    ) -> Vec<u8> {
        let mut data = Vec::with_capacity(4 + 4 + 8 + 8 + 8 + distributor_hash.len());
        data.extend_from_slice(&channel_id.to_be_bytes());
        data.extend_from_slice(&epoch.to_be_bytes());
        data.extend_from_slice(epoch_fp);
        data.extend_from_slice(parent_fp);
        data.extend_from_slice(&timestamp.to_be_bytes());
        data.extend_from_slice(distributor_hash.as_bytes());
        data
    }

    #[allow(clippy::too_many_arguments, reason = "protocol message construction requires all fields to be present")]
    fn build_key_exchange_signed_data(
        &self,
        algorithm_version: u8,
        channel_id: u32,
        mode: &crate::persistent::PersistenceMode,
        epoch: u32,
        encrypted_key: &[u8],
        recipient_hash: &str,
        request_id: Option<&str>,
        timestamp: u64,
    ) -> Vec<u8> {
        let mode_byte: u8 = match mode {
            crate::persistent::PersistenceMode::PostJoin => 1,
            crate::persistent::PersistenceMode::FullArchive => 2,
            _ => 0,
        };

        let req_id_bytes = request_id.unwrap_or("").as_bytes();
        let capacity = 1 + 4 + 1 + 4 + encrypted_key.len()
            + recipient_hash.len() + req_id_bytes.len() + 8;
        let mut data = Vec::with_capacity(capacity);
        data.push(algorithm_version);
        data.extend_from_slice(&channel_id.to_be_bytes());
        data.push(mode_byte);
        data.extend_from_slice(&epoch.to_be_bytes());
        data.extend_from_slice(encrypted_key);
        data.extend_from_slice(recipient_hash.as_bytes());
        data.extend_from_slice(req_id_bytes);
        data.extend_from_slice(&timestamp.to_be_bytes());
        data
    }

    fn build_key_announce_signed_data(
        &self,
        algorithm_version: u8,
        cert_hash: &str,
        timestamp: u64,
        identity_public: &[u8],
        signing_public: &[u8],
    ) -> Vec<u8> {
        let capacity = 1 + cert_hash.len() + 8 + identity_public.len() + signing_public.len();
        let mut data = Vec::with_capacity(capacity);
        data.push(algorithm_version);
        data.extend_from_slice(cert_hash.as_bytes());
        data.extend_from_slice(&timestamp.to_be_bytes());
        data.extend_from_slice(identity_public);
        data.extend_from_slice(signing_public);
        data
    }
}

// ---- Sha256Fingerprinter --------------------------------------------

/// Epoch fingerprinter using SHA-256.
#[derive(Debug, Clone, Default)]
pub struct Sha256Fingerprinter;

impl Fingerprinter for Sha256Fingerprinter {
    fn epoch_fingerprint(&self, key: &[u8]) -> [u8; 8] {
        use sha2::Digest;
        let hash = Sha256::digest(key);
        let mut fp = [0u8; 8];
        fp.copy_from_slice(&hash[..8]);
        fp
    }
}

// ---- HkdfArchiveKeyDeriver ------------------------------------------

/// Archive key deriver using HKDF-SHA256.
#[derive(Debug, Clone, Default)]
pub struct HkdfArchiveKeyDeriver;

impl ArchiveKeyDeriver for HkdfArchiveKeyDeriver {
    fn derive_archive_key(&self, seed: &[u8; 32], channel_id: u32) -> [u8; 32] {
        let hkdf = Hkdf::<Sha256>::new(Some(HKDF_SALT_ARCHIVE_KEY), seed);
        let mut key = [0u8; 32];
        #[allow(clippy::expect_used, reason = "HKDF-SHA256 expand with a 32-byte output can never fail")]
        hkdf.expand(&channel_id.to_be_bytes(), &mut key)
            .expect("HKDF expand for archive key");
        key
    }
}

// ---- Backward-compatible free functions ------------------------------
//
// These delegate to the default trait implementations above and are kept
// for callers that have not yet adopted the trait-based API.
// ---------------------------------------------------------------------

// ---- AAD construction -----------------------------------------------

/// Build the Associated Authenticated Data for AEAD.
///
/// Convenience wrapper around [`StandardAadBuilder`].
pub fn build_aad(channel_id: u32, message_id: &[u8; 16], timestamp: u64) -> Vec<u8> {
    StandardAadBuilder.build_aad(channel_id, message_id, timestamp)
}

/// Parse a UUID string into 16 raw bytes.
///
/// Convenience wrapper around [`StandardAadBuilder`].
pub fn uuid_to_bytes(uuid_str: &str) -> Result<[u8; 16]> {
    StandardAadBuilder.uuid_to_bytes(uuid_str)
}

// ---- Chain ratchet --------------------------------------------------

/// Derive the next chain key from the current one.
///
/// `chain_key[n+1] = HKDF-SHA256(ikm=chain_key[n], info="fancy-pchat-chain-v1")`
pub fn derive_chain_key(deriver: &dyn KeyDeriver, current: &[u8; KEY_LEN]) -> Result<[u8; KEY_LEN]> {
    deriver.derive(current, HKDF_SALT_IDENTITY, HKDF_INFO_CHAIN)
}

/// Derive a per-message key from a chain key.
///
/// `message_key[n] = HKDF-SHA256(ikm=chain_key[n], info="fancy-pchat-msg-v1")`
pub fn derive_message_key(deriver: &dyn KeyDeriver, chain_key: &[u8; KEY_LEN]) -> Result<[u8; KEY_LEN]> {
    deriver.derive(chain_key, HKDF_SALT_IDENTITY, HKDF_INFO_MSG)
}

/// Derive a chain key and message key at a specific index by ratcheting
/// forward from the epoch key.
///
/// This is used for historical message decryption when the intermediate
/// chain keys have been deleted.
pub fn derive_key_at_index(
    deriver: &dyn KeyDeriver,
    epoch_key: &[u8; KEY_LEN],
    target_index: u32,
) -> Result<[u8; KEY_LEN]> {
    let mut chain_key = *epoch_key;
    for _ in 0..target_index {
        chain_key = derive_chain_key(deriver, &chain_key)?;
    }
    derive_message_key(deriver, &chain_key)
}

// ---- Epoch fingerprint ----------------------------------------------

/// Compute the epoch fingerprint: `SHA-256(key)[0..8]`.
///
/// Convenience wrapper around [`Sha256Fingerprinter`].
pub fn epoch_fingerprint(key: &[u8]) -> [u8; 8] {
    Sha256Fingerprinter.epoch_fingerprint(key)
}

// ---- Padding --------------------------------------------------------

/// Pad plaintext using randomized block-aligned padding (section 10.1).
///
/// 1. Round up to next multiple of `PADDING_BLOCK` (256 bytes).
/// 2. Add random jitter of 0..255 bytes from CSPRNG.
/// 3. Append `pad_count` as 2-byte big-endian u16.
///
/// `pad_count` includes the 2-byte trailer itself (minimum value: 2).
fn pad_plaintext(plaintext: &[u8]) -> Result<Vec<u8>> {
    let min_padded_len = plaintext.len() + 2; // at least 2 bytes for pad_count
    let blocks_needed = min_padded_len.div_ceil(PADDING_BLOCK);
    let block_aligned = blocks_needed * PADDING_BLOCK;

    let jitter = (rand::rng().next_u32() % PADDING_BLOCK as u32) as usize;
    let padded_length = block_aligned + jitter;
    let pad_count = padded_length - plaintext.len();

    // pad_count must fit in u16
    let pad_count_u16: u16 = pad_count
        .try_into()
        .map_err(|_| Error::InvalidState("message too large for padding".into()))?;

    let mut output = Vec::with_capacity(padded_length);
    output.extend_from_slice(plaintext);
    // Zero padding bytes (pad_count - 2 of them)
    output.resize(padded_length - 2, 0x00);
    // pad_count trailer (2 bytes big-endian)
    output.extend_from_slice(&pad_count_u16.to_be_bytes());
    Ok(output)
}

/// Remove padding, recovering the original plaintext.
fn unpad_plaintext(padded: &[u8]) -> Result<Vec<u8>> {
    if padded.len() < 2 {
        return Err(Error::InvalidState("padded data too short".into()));
    }
    let pad_count =
        u16::from_be_bytes([padded[padded.len() - 2], padded[padded.len() - 1]]) as usize;

    if pad_count < 2 || pad_count > padded.len() {
        return Err(Error::InvalidState(format!(
            "invalid pad_count: {pad_count} for data length {}",
            padded.len()
        )));
    }

    let plaintext_len = padded.len() - pad_count;
    Ok(padded[..plaintext_len].to_vec())
}

// ---- Countersignature data builder ----------------------------------

/// Build the data that a key custodian signs for epoch countersignatures.
///
/// Convenience wrapper around [`StandardSignedDataBuilder`].
pub fn build_countersig_data(
    channel_id: u32,
    epoch: u32,
    epoch_fp: &[u8],
    parent_fp: &[u8],
    timestamp: u64,
    distributor_hash: &str,
) -> Vec<u8> {
    StandardSignedDataBuilder.build_countersig_data(
        channel_id, epoch, epoch_fp, parent_fp, timestamp, distributor_hash,
    )
}

/// Build the data signed in a key-exchange message (section 6.6).
///
/// Convenience wrapper around [`StandardSignedDataBuilder`].
#[allow(clippy::too_many_arguments, reason = "protocol message construction requires all fields to be present")]
pub fn build_key_exchange_signed_data(
    algorithm_version: u8,
    channel_id: u32,
    mode: &crate::persistent::PersistenceMode,
    epoch: u32,
    encrypted_key: &[u8],
    recipient_hash: &str,
    request_id: Option<&str>,
    timestamp: u64,
) -> Vec<u8> {
    StandardSignedDataBuilder.build_key_exchange_signed_data(
        algorithm_version, channel_id, mode, epoch,
        encrypted_key, recipient_hash, request_id, timestamp,
    )
}

/// Build the data signed in a key-announce message (section 6.8).
///
/// Convenience wrapper around [`StandardSignedDataBuilder`].
pub fn build_key_announce_signed_data(
    algorithm_version: u8,
    cert_hash: &str,
    timestamp: u64,
    identity_public: &[u8],
    signing_public: &[u8],
) -> Vec<u8> {
    StandardSignedDataBuilder.build_key_announce_signed_data(
        algorithm_version, cert_hash, timestamp, identity_public, signing_public,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn pad_unpad_roundtrip() {
        let plaintext = b"Hello, world!";
        let padded = pad_plaintext(plaintext).unwrap();
        assert!(padded.len() >= plaintext.len() + 2);
        assert!(padded.len().is_multiple_of(PADDING_BLOCK) || padded.len() > PADDING_BLOCK);
        let recovered = unpad_plaintext(&padded).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn pad_minimum_size() {
        let padded = pad_plaintext(b"").unwrap();
        // minimum: 1 block (256) + 0..255 jitter
        assert!(padded.len() >= PADDING_BLOCK);
        let recovered = unpad_plaintext(&padded).unwrap();
        assert!(recovered.is_empty());
    }

    #[test]
    fn unpad_rejects_short_data() {
        assert!(unpad_plaintext(&[0]).is_err());
        assert!(unpad_plaintext(&[]).is_err());
    }

    #[test]
    fn unpad_rejects_invalid_pad_count() {
        // pad_count = 0 (invalid, minimum is 2)
        assert!(unpad_plaintext(&[0x00, 0x00]).is_err());
        // pad_count = 1 (invalid, minimum is 2)
        assert!(unpad_plaintext(&[0x00, 0x01]).is_err());
    }

    #[test]
    fn xchacha_encrypt_decrypt_roundtrip() {
        let enc = XChaChaEncryptor;
        let key = [0x42u8; KEY_LEN];
        let plaintext = b"Secret message for testing";
        let aad = build_aad(1, &[0u8; 16], 1234567890);

        let ciphertext = enc.encrypt(&key, plaintext, &aad).unwrap();
        assert_ne!(&ciphertext[1 + NONCE_LEN..], plaintext);

        let decrypted = enc.decrypt(&key, &ciphertext, &aad).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn xchacha_wrong_key_fails() {
        let enc = XChaChaEncryptor;
        let key = [0x42u8; KEY_LEN];
        let wrong_key = [0x43u8; KEY_LEN];
        let aad = build_aad(1, &[0u8; 16], 0);

        let ciphertext = enc.encrypt(&key, b"test", &aad).unwrap();
        assert!(enc.decrypt(&wrong_key, &ciphertext, &aad).is_err());
    }

    #[test]
    fn xchacha_wrong_aad_fails() {
        let enc = XChaChaEncryptor;
        let key = [0x42u8; KEY_LEN];
        let aad1 = build_aad(1, &[0u8; 16], 0);
        let aad2 = build_aad(2, &[0u8; 16], 0); // different channel

        let ciphertext = enc.encrypt(&key, b"test", &aad1).unwrap();
        assert!(enc.decrypt(&key, &ciphertext, &aad2).is_err());
    }

    #[test]
    fn xchacha_version_byte() {
        let enc = XChaChaEncryptor;
        let key = [0x42u8; KEY_LEN];
        let ciphertext = enc.encrypt(&key, b"test", &[]).unwrap();
        assert_eq!(ciphertext[0], ENCRYPTION_VERSION);
    }

    #[test]
    fn xchacha_rejects_wrong_version() {
        let enc = XChaChaEncryptor;
        let key = [0x42u8; KEY_LEN];
        let mut ciphertext = enc.encrypt(&key, b"test", &[]).unwrap();
        ciphertext[0] = 0xFF; // corrupt version
        assert!(enc.decrypt(&key, &ciphertext, &[]).is_err());
    }

    #[test]
    fn hkdf_derive_produces_key() {
        let deriver = HkdfSha256Deriver;
        let ikm = [0xAA; 32];
        let key = deriver.derive(&ikm, b"salt", b"info").unwrap();
        assert_eq!(key.len(), KEY_LEN);
        assert_ne!(key, ikm); // derived key differs from input
    }

    #[test]
    fn chain_ratchet_deterministic() {
        let deriver = HkdfSha256Deriver;
        let epoch_key = [0x01u8; KEY_LEN];

        let chain1 = derive_chain_key(&deriver, &epoch_key).unwrap();
        let chain1_again = derive_chain_key(&deriver, &epoch_key).unwrap();
        assert_eq!(chain1, chain1_again);

        let msg_key = derive_message_key(&deriver, &chain1).unwrap();
        assert_ne!(msg_key, chain1);
    }

    #[test]
    fn derive_key_at_index_matches_sequential() {
        let deriver = HkdfSha256Deriver;
        let epoch_key = [0x55u8; KEY_LEN];

        // Derive sequentially
        let mut chain = epoch_key;
        for _ in 0..5 {
            chain = derive_chain_key(&deriver, &chain).unwrap();
        }
        let expected = derive_message_key(&deriver, &chain).unwrap();

        // Derive at index
        let at_index = derive_key_at_index(&deriver, &epoch_key, 5).unwrap();
        assert_eq!(at_index, expected);
    }

    #[test]
    fn epoch_fingerprint_deterministic() {
        let key = [0xBB; 32];
        let fp1 = epoch_fingerprint(&key);
        let fp2 = epoch_fingerprint(&key);
        assert_eq!(fp1, fp2);
        assert_eq!(fp1.len(), 8);
    }

    #[test]
    fn epoch_fingerprint_differs_for_different_keys() {
        let fp1 = epoch_fingerprint(&[0x01; 32]);
        let fp2 = epoch_fingerprint(&[0x02; 32]);
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn build_aad_format() {
        let aad = build_aad(1, &[0u8; 16], 100);
        assert_eq!(aad.len(), 4 + 16 + 8);
        // channel_id = 1 big-endian
        assert_eq!(&aad[..4], &[0, 0, 0, 1]);
        // timestamp = 100 big-endian
        assert_eq!(&aad[20..], &100u64.to_be_bytes());
    }

    #[test]
    fn key_exchange_signed_data_includes_all_fields() {
        let data = build_key_exchange_signed_data(
            1,
            42,
            &crate::persistent::PersistenceMode::PostJoin,
            5,
            &[0xAA; 48],
            "recipient",
            Some("req-1"),
            12345,
        );
        // Should contain: version(1) + channel(4) + mode(1) + epoch(4) + key(48) + recipient(9) + req_id(5) + ts(8)
        assert_eq!(data.len(), 1 + 4 + 1 + 4 + 48 + 9 + 5 + 8);
        assert_eq!(data[0], 1); // algorithm_version
    }

    #[test]
    fn key_announce_signed_data_format() {
        let data = build_key_announce_signed_data(1, "abc123", 99999, &[0; 32], &[0; 32]);
        // version(1) + cert_hash(6) + ts(8) + id_pub(32) + sign_pub(32) = 79
        assert_eq!(data.len(), 1 + 6 + 8 + 32 + 32);
        assert_eq!(data[0], 1);
    }

    #[test]
    fn countersig_data_format() {
        let data = build_countersig_data(1, 2, &[0; 8], &[0; 8], 5000, "dist");
        // channel(4) + epoch(4) + efp(8) + pfp(8) + ts(8) + dist(4) = 36
        assert_eq!(data.len(), 4 + 4 + 8 + 8 + 8 + 4);
    }
}
