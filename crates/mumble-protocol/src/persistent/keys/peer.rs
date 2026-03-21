//! Peer key management and key announcement generation.

use ed25519_dalek::Verifier;
use fancy_utils::hex::bytes_to_hex;
use x25519_dalek::PublicKey as X25519PublicKey;

use crate::error::{Error, Result};
use crate::persistent::encryption;
use crate::persistent::wire::PchatKeyAnnounce;

use super::types::{ALGORITHM_VERSION, PeerKeyRecord};
use super::KeyManager;

impl KeyManager {
    // ---- Peer key management ----------------------------------------

    /// Record a peer's public keys from a `fancy-pchat-key-announce`.
    ///
    /// Enforces anti-rollback: discards announcements with timestamp
    /// <= the known highest for this peer (section 6.8).
    pub fn record_peer_key(&mut self, announce: &PchatKeyAnnounce) -> Result<bool> {
        if announce.algorithm_version != ALGORITHM_VERSION {
            return Err(Error::InvalidState(format!(
                "unsupported algorithm_version: {}",
                announce.algorithm_version
            )));
        }

        if announce.identity_public.len() != 32 || announce.signing_public.len() != 32 {
            tracing::warn!(
                cert_hash = %announce.cert_hash,
                id_pub_len = announce.identity_public.len(),
                sign_pub_len = announce.signing_public.len(),
                sig_len = announce.signature.len(),
                "key-announce has invalid key lengths (expected 32, 32, 64) \
                 -- possible BLOB truncation by server DB"
            );
            return Err(Error::InvalidState("invalid key lengths".into()));
        }

        // Verify Ed25519 self-signature
        let signed_data = encryption::build_key_announce_signed_data(
            announce.algorithm_version,
            &announce.cert_hash,
            announce.timestamp,
            &announce.identity_public,
            &announce.signing_public,
        );

        let signing_bytes: [u8; 32] = announce.signing_public[..32]
            .try_into()
            .map_err(|_| Error::InvalidState("invalid signing key".into()))?;
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&signing_bytes)
            .map_err(|e| Error::InvalidState(format!("invalid Ed25519 key: {e}")))?;
        let signature = ed25519_dalek::Signature::from_slice(&announce.signature)
            .map_err(|e| Error::InvalidState(format!("invalid signature: {e}")))?;

        verifying_key
            .verify(&signed_data, &signature)
            .map_err(|e| {
                let sig_hex = bytes_to_hex(&announce.signature);
                tracing::warn!(
                    cert_hash = %announce.cert_hash,
                    timestamp = announce.timestamp,
                    signed_data_len = signed_data.len(),
                    sig_hex,
                    "key-announce signature verification failed: {e}"
                );
                Error::InvalidState(format!("signature verification failed: {e}"))
            })?;

        // Anti-rollback check
        if let Some(existing) = self.peer_keys.get(&announce.cert_hash) {
            if announce.timestamp <= existing.highest_announce_ts {
                return Ok(false); // silently discard stale announcement
            }
        }

        let dh_bytes: [u8; 32] = announce.identity_public[..32]
            .try_into()
            .map_err(|_| Error::InvalidState("invalid DH key".into()))?;

        let _ = self.peer_keys.insert(
            announce.cert_hash.clone(),
            PeerKeyRecord {
                algorithm_version: announce.algorithm_version,
                dh_public: X25519PublicKey::from(dh_bytes),
                signing_public: verifying_key,
                highest_announce_ts: announce.timestamp,
            },
        );

        Ok(true)
    }

    /// Look up a peer's known keys.
    pub fn get_peer(&self, cert_hash: &str) -> Option<&PeerKeyRecord> {
        self.peer_keys.get(cert_hash)
    }

    // ---- Key announcement generation --------------------------------

    /// Build a `fancy-pchat-key-announce` payload for our identity.
    pub fn build_key_announce(&self, cert_hash: &str, timestamp: u64) -> PchatKeyAnnounce {
        let identity_public = self.dh_public_bytes().to_vec();
        let signing_public = self.signing_public_bytes().to_vec();

        let signed_data = encryption::build_key_announce_signed_data(
            ALGORITHM_VERSION,
            cert_hash,
            timestamp,
            &identity_public,
            &signing_public,
        );
        let signature = self.identity.sign(&signed_data);

        PchatKeyAnnounce {
            algorithm_version: ALGORITHM_VERSION,
            identity_public,
            signing_public,
            cert_hash: cert_hash.to_string(),
            timestamp,
            signature: signature.to_bytes().to_vec(),
            tls_signature: Vec::new(), // filled in by the caller (requires TLS private key)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::super::identity::SeedIdentity;
    use super::super::types::ALGORITHM_VERSION;
    use super::super::KeyManager;

    fn make_key_manager() -> KeyManager {
        let identity = SeedIdentity::from_seed(&[0xAA; 32]).unwrap();
        KeyManager::new(Box::new(identity))
    }

    #[test]
    fn key_announce_roundtrip() {
        let km = make_key_manager();
        let announce = km.build_key_announce("test_cert", 12345);
        assert_eq!(announce.algorithm_version, ALGORITHM_VERSION);
        assert_eq!(announce.cert_hash, "test_cert");
        assert_eq!(announce.identity_public.len(), 32);
        assert_eq!(announce.signing_public.len(), 32);

        // Another km can verify and record the peer
        let mut km2 = make_key_manager();
        let result = km2.record_peer_key(&announce);
        assert!(result.is_ok());
        assert!(km2.get_peer("test_cert").is_some());
    }

    #[test]
    fn anti_rollback_rejects_stale_announce() {
        let km = make_key_manager();
        let announce1 = km.build_key_announce("peer1", 100);
        let announce2 = km.build_key_announce("peer1", 50); // older timestamp

        let mut km2 = make_key_manager();
        assert!(km2.record_peer_key(&announce1).unwrap());
        // Stale announcement should be silently discarded
        assert!(!km2.record_peer_key(&announce2).unwrap());
    }
}
