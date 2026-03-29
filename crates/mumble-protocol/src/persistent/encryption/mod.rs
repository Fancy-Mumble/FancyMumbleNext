//! Re-exports from the v1 protocol implementation.
//!
//! The actual implementation lives in
//! [`protocol::fancy_v1`](super::protocol::fancy_v1). This module
//! re-exports everything for backward compatibility so existing code
//! can keep using `crate::persistent::encryption::*`.

pub use super::protocol::fancy_v1::aad;
pub use super::protocol::fancy_v1::archive_key;
pub use super::protocol::fancy_v1::chain_ratchet;
pub use super::protocol::fancy_v1::encryptor;
pub use super::protocol::fancy_v1::fingerprint;
pub use super::protocol::fancy_v1::key_deriver;
pub use super::protocol::fancy_v1::signed_data;
pub use super::protocol::fancy_v1::suite;

pub use super::protocol::fancy_v1::{
    ENCRYPTION_VERSION, HKDF_INFO_CHAIN, HKDF_INFO_ED25519, HKDF_INFO_MSG, HKDF_INFO_X25519,
    HKDF_SALT_ARCHIVE_KEY, HKDF_SALT_IDENTITY, KEY_LEN,
};

pub use super::protocol::fancy_v1::{
    build_aad, build_countersig_data, build_key_announce_signed_data,
    build_key_exchange_signed_data, derive_archive_key, derive_chain_key, derive_key_at_index,
    derive_message_key, epoch_fingerprint, uuid_to_bytes, AadBuilder, ArchiveKeyDeriver,
    ChainRatchet, CryptoSuite, Encryptor, Fingerprinter, HkdfArchiveKeyDeriver, HkdfChainRatchet,
    HkdfSha256Deriver, KeyDeriver, Sha256Fingerprinter, SignedDataBuilder,
    StandardAadBuilder, StandardSignedDataBuilder, XChaCha20Suite, XChaChaEncryptor,
};
