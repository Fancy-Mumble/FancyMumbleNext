//! Message encryption and decryption for persistent chat.

use crate::error::{Error, Result};
use crate::persistent::encryption;
use crate::persistent::PchatProtocol;

use super::types::EncryptedPayload;
use super::KeyManager;

impl KeyManager {
    // ---- Encryption / Decryption ------------------------------------

    /// Encrypt a message for the given protocol and channel.
    pub fn encrypt(
        &mut self,
        protocol: PchatProtocol,
        channel_id: u32,
        message_id: &str,
        timestamp: u64,
        plaintext: &[u8],
    ) -> Result<EncryptedPayload> {
        let uuid_bytes = encryption::uuid_to_bytes(message_id)?;
        let aad = encryption::build_aad(channel_id, &uuid_bytes, timestamp);

        match protocol {
            PchatProtocol::FancyV1PostJoin => {
                let epochs = self
                    .epoch_keys
                    .get_mut(&channel_id)
                    .ok_or_else(|| Error::InvalidState("no epoch keys for channel".into()))?;
                let (&current_epoch, entry) = epochs
                    .iter_mut()
                    .next_back()
                    .ok_or_else(|| Error::InvalidState("no current epoch".into()))?;
                let (epoch_key, _trust) = entry;

                let msg_key =
                    encryption::derive_message_key(self.suite.key_deriver(), &epoch_key.current_chain_key)?;
                let chain_index = epoch_key.chain_index;

                // Ratchet chain forward
                epoch_key.current_chain_key =
                    encryption::derive_chain_key(self.suite.key_deriver(), &epoch_key.current_chain_key)?;
                epoch_key.chain_index += 1;

                let ciphertext = self.suite.encryptor().encrypt(&msg_key, plaintext, &aad)?;
                let fp = epoch_key.fingerprint();

                Ok(EncryptedPayload {
                    ciphertext,
                    epoch: Some(current_epoch),
                    chain_index: Some(chain_index),
                    epoch_fingerprint: fp,
                })
            }
            PchatProtocol::FancyV1FullArchive => {
                let (channel_key, _trust) = self
                    .archive_keys
                    .get(&channel_id)
                    .ok_or_else(|| Error::InvalidState("no archive key for channel".into()))?;

                let ciphertext = self.suite.encryptor().encrypt(&channel_key.key, plaintext, &aad)?;
                let fp = channel_key.fingerprint();

                Ok(EncryptedPayload {
                    ciphertext,
                    epoch: None,
                    chain_index: None,
                    epoch_fingerprint: fp,
                })
            }
            _ => Err(Error::InvalidState(format!(
                "cannot encrypt for protocol {protocol:?}"
            ))),
        }
    }

    /// Decrypt a message.
    pub fn decrypt(
        &self,
        protocol: PchatProtocol,
        channel_id: u32,
        message_id: &str,
        timestamp: u64,
        payload: &EncryptedPayload,
    ) -> Result<Vec<u8>> {
        let uuid_bytes = encryption::uuid_to_bytes(message_id)?;
        let aad = encryption::build_aad(channel_id, &uuid_bytes, timestamp);

        match protocol {
            PchatProtocol::FancyV1PostJoin => {
                let epoch = payload
                    .epoch
                    .ok_or_else(|| Error::InvalidState("missing epoch for POST_JOIN".into()))?;
                let chain_idx = payload
                    .chain_index
                    .ok_or_else(|| Error::InvalidState("missing chain_index".into()))?;

                let epochs = self
                    .epoch_keys
                    .get(&channel_id)
                    .ok_or_else(|| Error::InvalidState("no epoch keys for channel".into()))?;
                let (epoch_key, _trust) = epochs
                    .get(&epoch)
                    .ok_or_else(|| Error::InvalidState(format!("unknown epoch: {epoch}")))?;

                // Re-derive the message key at the specified chain index
                let msg_key =
                    encryption::derive_key_at_index(self.suite.key_deriver(), &epoch_key.key, chain_idx)?;

                self.suite.encryptor().decrypt(&msg_key, &payload.ciphertext, &aad)
            }
            PchatProtocol::FancyV1FullArchive => {
                let (channel_key, _trust) = self
                    .archive_keys
                    .get(&channel_id)
                    .ok_or_else(|| Error::InvalidState("no archive key for channel".into()))?;

                self.suite.encryptor()
                    .decrypt(&channel_key.key, &payload.ciphertext, &aad)
            }
            _ => Err(Error::InvalidState(format!(
                "cannot decrypt for protocol {protocol:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::super::identity::SeedIdentity;
    use super::super::KeyManager;
    use crate::persistent::{KeyTrustLevel, PchatProtocol};

    fn make_key_manager() -> KeyManager {
        let identity = SeedIdentity::from_seed(&[0xAA; 32]).unwrap();
        KeyManager::new(Box::new(identity))
    }

    #[test]
    fn encrypt_decrypt_full_archive() {
        let mut km = make_key_manager();
        let key = [0x42u8; 32];
        km.store_archive_key(1, key, KeyTrustLevel::Verified);

        let msg_id = uuid::Uuid::new_v4().to_string();
        let plaintext = b"Hello, world!";
        let payload = km
            .encrypt(PchatProtocol::FancyV1FullArchive, 1, &msg_id, 1000, plaintext)
            .unwrap();

        let decrypted = km
            .decrypt(PchatProtocol::FancyV1FullArchive, 1, &msg_id, 1000, &payload)
            .unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_decrypt_post_join() {
        let mut km = make_key_manager();
        let key = [0x55u8; 32];
        km.store_epoch_key(1, 0, key, KeyTrustLevel::Verified);

        let msg_id = uuid::Uuid::new_v4().to_string();
        let plaintext = b"Epoch message";
        let payload = km
            .encrypt(PchatProtocol::FancyV1PostJoin, 1, &msg_id, 2000, plaintext)
            .unwrap();

        assert_eq!(payload.epoch, Some(0));
        assert_eq!(payload.chain_index, Some(0));

        let decrypted = km
            .decrypt(PchatProtocol::FancyV1PostJoin, 1, &msg_id, 2000, &payload)
            .unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn chain_ratchet_advances() {
        let mut km = make_key_manager();
        km.store_epoch_key(1, 0, [0x55; 32], KeyTrustLevel::Verified);

        let id1 = uuid::Uuid::new_v4().to_string();
        let p1 = km
            .encrypt(PchatProtocol::FancyV1PostJoin, 1, &id1, 100, b"msg1")
            .unwrap();
        assert_eq!(p1.chain_index, Some(0));

        let id2 = uuid::Uuid::new_v4().to_string();
        let p2 = km
            .encrypt(PchatProtocol::FancyV1PostJoin, 1, &id2, 200, b"msg2")
            .unwrap();
        assert_eq!(p2.chain_index, Some(1));

        // Both should decrypt correctly
        assert_eq!(
            km.decrypt(PchatProtocol::FancyV1PostJoin, 1, &id1, 100, &p1)
                .unwrap(),
            b"msg1"
        );
        assert_eq!(
            km.decrypt(PchatProtocol::FancyV1PostJoin, 1, &id2, 200, &p2)
                .unwrap(),
            b"msg2"
        );
    }
}
