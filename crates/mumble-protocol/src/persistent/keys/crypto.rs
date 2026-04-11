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
            PchatProtocol::SignalV1 => {
                let bridge = self
                    .signal_bridge
                    .as_ref()
                    .ok_or_else(|| Error::InvalidState("signal bridge not loaded".into()))?;

                let ciphertext = bridge.group_encrypt(channel_id, plaintext)?;

                Ok(EncryptedPayload {
                    ciphertext,
                    epoch: None,
                    chain_index: None,
                    epoch_fingerprint: [0u8; 8],
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

    /// Decrypt a `SignalV1` message from a specific sender.
    ///
    /// `SignalV1` uses per-sender keys (Sender Key groups) so the
    /// sender's cert hash is required for decryption.
    pub fn decrypt_signal(
        &self,
        sender_hash: &str,
        channel_id: u32,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>> {
        let bridge = self
            .signal_bridge
            .as_ref()
            .ok_or_else(|| Error::InvalidState("signal bridge not loaded".into()))?;

        bridge.group_decrypt(sender_hash, channel_id, ciphertext)
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
}
