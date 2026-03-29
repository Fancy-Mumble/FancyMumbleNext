//! Fancy Mumble v1 E2EE protocol implementation.
//!
//! XChaCha20-Poly1305 AEAD + HKDF-SHA256 key derivation,
//! X25519 key agreement, Ed25519 signing.
//!
//! Protocol `algorithm_version` byte: `0x01`.

pub mod aad;
pub mod archive_key;
pub mod chain_ratchet;
pub mod encryptor;
pub mod fingerprint;
pub mod identity;
pub mod key_deriver;
pub mod signed_data;
pub mod suite;

// ---- Constants ------------------------------------------------------

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

/// XChaCha20-Poly1305 key length.
pub const KEY_LEN: usize = 32;

// ---- Re-exports -----------------------------------------------------

pub use aad::{build_aad, uuid_to_bytes, AadBuilder, StandardAadBuilder};
pub use archive_key::{derive_archive_key, ArchiveKeyDeriver, HkdfArchiveKeyDeriver};
pub use chain_ratchet::{
    derive_chain_key, derive_key_at_index, derive_message_key, ChainRatchet, HkdfChainRatchet,
};
pub use encryptor::{Encryptor, XChaChaEncryptor};
pub use fingerprint::{epoch_fingerprint, Fingerprinter, Sha256Fingerprinter};
pub use identity::{CryptoIdentity, SeedIdentity};
pub use key_deriver::{HkdfSha256Deriver, KeyDeriver};
pub use signed_data::{
    build_countersig_data, build_key_announce_signed_data, build_key_exchange_signed_data,
    SignedDataBuilder, StandardSignedDataBuilder,
};
pub use suite::{CryptoSuite, XChaCha20Suite};
