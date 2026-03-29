//! Epoch key fingerprint trait and SHA-256 implementation.

use sha2::Sha256;

// ---- Trait: Fingerprinter -------------------------------------------

/// Trait abstracting epoch key fingerprint computation.
pub trait Fingerprinter: Send + Sync {
    /// Compute a short fingerprint of a key.
    fn epoch_fingerprint(&self, key: &[u8]) -> [u8; 8];
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

// ---- Convenience free function --------------------------------------

/// Compute the epoch fingerprint: `SHA-256(key)[0..8]`.
///
/// Convenience wrapper around [`Sha256Fingerprinter`].
pub fn epoch_fingerprint(key: &[u8]) -> [u8; 8] {
    Sha256Fingerprinter.epoch_fingerprint(key)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
