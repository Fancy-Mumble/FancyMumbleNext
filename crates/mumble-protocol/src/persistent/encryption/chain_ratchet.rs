//! Symmetric chain ratchet trait and HKDF-based implementation.

use crate::error::Result;

use super::key_deriver::{HkdfSha256Deriver, KeyDeriver};
use super::{HKDF_INFO_CHAIN, HKDF_INFO_MSG, HKDF_SALT_IDENTITY, KEY_LEN};

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

// ---- Backward-compatible free functions -----------------------------

/// Derive the next chain key from the current one.
///
/// `chain_key[n+1] = HKDF-SHA256(ikm=chain_key[n], info="fancy-pchat-chain-v1")`
pub fn derive_chain_key(
    deriver: &dyn KeyDeriver,
    current: &[u8; KEY_LEN],
) -> Result<[u8; KEY_LEN]> {
    deriver.derive(current, HKDF_SALT_IDENTITY, HKDF_INFO_CHAIN)
}

/// Derive a per-message key from a chain key.
///
/// `message_key[n] = HKDF-SHA256(ikm=chain_key[n], info="fancy-pchat-msg-v1")`
pub fn derive_message_key(
    deriver: &dyn KeyDeriver,
    chain_key: &[u8; KEY_LEN],
) -> Result<[u8; KEY_LEN]> {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

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
}
