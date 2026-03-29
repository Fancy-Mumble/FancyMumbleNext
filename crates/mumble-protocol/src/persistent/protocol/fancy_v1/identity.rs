//! Cryptographic identity trait and seed-based implementation.

use ed25519_dalek::Signer;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

use crate::error::Result;
use crate::persistent::encryption::{self, HkdfSha256Deriver, KeyDeriver};

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

// ---- SeedIdentity ---------------------------------------------------

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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn seed_identity_deterministic() {
        let seed = [0xAA; 32];
        let id1 = SeedIdentity::from_seed(&seed).unwrap();
        let id2 = SeedIdentity::from_seed(&seed).unwrap();
        assert_eq!(
            id1.dh_public_key().as_bytes(),
            id2.dh_public_key().as_bytes()
        );
        assert_eq!(
            id1.signing_public_key().to_bytes(),
            id2.signing_public_key().to_bytes()
        );
    }

    #[test]
    fn different_seeds_different_keys() {
        let id1 = SeedIdentity::from_seed(&[0x01; 32]).unwrap();
        let id2 = SeedIdentity::from_seed(&[0x02; 32]).unwrap();
        assert_ne!(
            id1.dh_public_key().as_bytes(),
            id2.dh_public_key().as_bytes()
        );
    }
}
