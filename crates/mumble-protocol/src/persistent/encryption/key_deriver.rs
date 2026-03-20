//! Key derivation trait and HKDF-SHA256 implementation.

use crate::error::Result;
use hkdf::Hkdf;
use sha2::Sha256;

use super::KEY_LEN;

// ---- Trait: KeyDeriver ----------------------------------------------

/// Trait abstracting key derivation (HKDF).
///
/// Separated so that key derivation logic can be tested independently.
pub trait KeyDeriver: Send + Sync {
    /// Derive a fixed-length key from input key material.
    fn derive(&self, ikm: &[u8], salt: &[u8], info: &[u8]) -> Result<[u8; KEY_LEN]>;
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
            .map_err(|e| crate::error::Error::Other(format!("HKDF expand failed: {e}")))?;
        Ok(okm)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn hkdf_derive_produces_key() {
        let deriver = HkdfSha256Deriver;
        let ikm = [0xAA; 32];
        let key = deriver.derive(&ikm, b"salt", b"info").unwrap();
        assert_eq!(key.len(), KEY_LEN);
        assert_ne!(key, ikm); // derived key differs from input
    }
}
