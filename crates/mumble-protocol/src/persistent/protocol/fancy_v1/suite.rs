//! Cipher suite trait and built-in implementations.
//!
//! A [`CryptoSuite`] bundles all symmetric cryptographic primitives
//! (AEAD encryption, key derivation, chain ratchet, fingerprinting,
//! archive key derivation, AAD construction, signed data layout) into
//! a single interchangeable unit.
//!
//! Consumers use the suite through trait methods, making the concrete
//! encryption algorithm transparent. To add a new E2EE scheme (e.g.
//! Signal Double Ratchet), implement [`CryptoSuite`] and pass the new
//! suite to [`KeyManager::with_suite`](crate::persistent::keys::KeyManager::with_suite).

use super::aad::{AadBuilder, StandardAadBuilder};
use super::archive_key::{ArchiveKeyDeriver, HkdfArchiveKeyDeriver};
use super::chain_ratchet::{ChainRatchet, HkdfChainRatchet};
use super::encryptor::{Encryptor, XChaChaEncryptor};
use super::fingerprint::{Fingerprinter, Sha256Fingerprinter};
use super::key_deriver::{HkdfSha256Deriver, KeyDeriver};
use super::signed_data::{SignedDataBuilder, StandardSignedDataBuilder};

// ---- Trait: CryptoSuite ---------------------------------------------

/// A complete cipher suite bundling all symmetric crypto primitives.
///
/// Each method returns a reference to a trait object so the caller
/// never depends on a concrete implementation. Implementations are
/// expected to return lightweight, zero-allocation references (e.g.
/// `&self.encryptor` or static references to unit structs).
pub trait CryptoSuite: Send + Sync {
    /// Version byte embedded in encrypted payloads.
    ///
    /// Used to select the correct suite for decryption.
    fn version(&self) -> u8;

    /// Human-readable name for logging and diagnostics.
    fn name(&self) -> &'static str;

    /// Symmetric AEAD encryption and decryption.
    fn encryptor(&self) -> &dyn Encryptor;

    /// Key derivation (HKDF or equivalent).
    fn key_deriver(&self) -> &dyn KeyDeriver;

    /// Symmetric chain ratchet for forward secrecy.
    fn chain_ratchet(&self) -> &dyn ChainRatchet;

    /// AAD construction for AEAD.
    fn aad_builder(&self) -> &dyn AadBuilder;

    /// Key fingerprint computation.
    fn fingerprinter(&self) -> &dyn Fingerprinter;

    /// Deterministic archive key derivation from seed + channel ID.
    fn archive_key_deriver(&self) -> &dyn ArchiveKeyDeriver;

    /// Byte-layout builder for signed protocol messages.
    fn signed_data_builder(&self) -> &dyn SignedDataBuilder;
}

// ---- XChaCha20Suite -------------------------------------------------

/// Production suite: XChaCha20-Poly1305 + HKDF-SHA256.
///
/// This is the default and currently only cipher suite (version `0x01`).
#[derive(Debug, Clone, Default)]
pub struct XChaCha20Suite;

impl CryptoSuite for XChaCha20Suite {
    fn version(&self) -> u8 {
        super::ENCRYPTION_VERSION
    }

    fn name(&self) -> &'static str {
        "XChaCha20-Poly1305 + HKDF-SHA256"
    }

    fn encryptor(&self) -> &dyn Encryptor {
        &XChaChaEncryptor
    }

    fn key_deriver(&self) -> &dyn KeyDeriver {
        &HkdfSha256Deriver
    }

    fn chain_ratchet(&self) -> &dyn ChainRatchet {
        &HkdfChainRatchet
    }

    fn aad_builder(&self) -> &dyn AadBuilder {
        &StandardAadBuilder
    }

    fn fingerprinter(&self) -> &dyn Fingerprinter {
        &Sha256Fingerprinter
    }

    fn archive_key_deriver(&self) -> &dyn ArchiveKeyDeriver {
        &HkdfArchiveKeyDeriver
    }

    fn signed_data_builder(&self) -> &dyn SignedDataBuilder {
        &StandardSignedDataBuilder
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistent::encryption::ENCRYPTION_VERSION;

    #[test]
    fn xchacha20_suite_version() {
        let suite = XChaCha20Suite;
        assert_eq!(suite.version(), ENCRYPTION_VERSION);
    }

    #[test]
    fn xchacha20_suite_name() {
        let suite = XChaCha20Suite;
        assert!(!suite.name().is_empty());
    }

    #[test]
    #[allow(clippy::expect_used, reason = "test code - panicking on failure is acceptable")]
    fn xchacha20_suite_components_are_accessible() {
        let suite = XChaCha20Suite;
        // Verify all accessors return valid trait objects
        assert_eq!(suite.encryptor().version(), ENCRYPTION_VERSION);
        let key = suite
            .key_deriver()
            .derive(&[0xAA; 32], b"salt", b"info")
            .expect("key derivation must succeed");
        assert_eq!(key.len(), 32);
    }
}
