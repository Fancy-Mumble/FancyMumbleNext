//! Archive key derivation trait and HKDF-SHA256 implementation.

use hkdf::Hkdf;
use sha2::Sha256;

use super::HKDF_SALT_ARCHIVE_KEY;

// ---- Trait: ArchiveKeyDeriver ----------------------------------------

/// Trait abstracting deterministic archive key derivation from
/// an identity seed and channel ID.
pub trait ArchiveKeyDeriver: Send + Sync {
    /// Derive a deterministic archive key.
    fn derive_archive_key(&self, seed: &[u8; 32], channel_id: u32) -> [u8; 32];
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

// ---- Convenience free function --------------------------------------

/// Derive a deterministic archive key from the identity seed and channel ID.
///
/// Convenience wrapper around [`HkdfArchiveKeyDeriver`].
pub fn derive_archive_key(seed: &[u8; 32], channel_id: u32) -> [u8; 32] {
    HkdfArchiveKeyDeriver.derive_archive_key(seed, channel_id)
}
