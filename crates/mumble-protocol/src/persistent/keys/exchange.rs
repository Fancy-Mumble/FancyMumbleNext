//! Key exchange protocol: signature verification, receive, distribute, handle requests.

use std::collections::HashMap;
use std::time::Instant;

use ed25519_dalek::Verifier;
use x25519_dalek::PublicKey as X25519PublicKey;

use crate::error::{Error, Result};
use crate::persistent::encryption::{
    self, build_key_exchange_signed_data, epoch_fingerprint,
};
use crate::persistent::wire::{PchatKeyExchange, PchatKeyRequest};
use crate::persistent::PchatProtocol;

use super::types::{ALGORITHM_VERSION, KEY_EXCHANGE_FRESHNESS_MS, ChannelKey, ConsensusCollector};
use super::KeyManager;
use crate::persistent::KeyTrustLevel;

impl KeyManager {
    // ---- Key exchange signature verification ------------------------

    /// Verify the Ed25519 signature on a key-exchange payload.
    pub fn verify_key_exchange_signature(&self, exchange: &PchatKeyExchange) -> Result<()> {
        let peer = self.peer_keys.get(&exchange.sender_hash).ok_or_else(|| {
            Error::InvalidState(format!("unknown sender: {}", exchange.sender_hash))
        })?;

        if exchange.algorithm_version != peer.algorithm_version {
            return Err(Error::InvalidState(
                "algorithm_version mismatch with sender's announced version".into(),
            ));
        }

        let protocol = PchatProtocol::from_wire_str(&exchange.protocol);
        let signed_data = build_key_exchange_signed_data(
            exchange.algorithm_version,
            exchange.channel_id,
            &protocol,
            exchange.epoch,
            &exchange.encrypted_key,
            &exchange.recipient_hash,
            exchange.request_id.as_deref(),
            exchange.timestamp,
        );

        let signature = ed25519_dalek::Signature::from_slice(&exchange.signature)
            .map_err(|e| Error::InvalidState(format!("invalid signature bytes: {e}")))?;

        peer.signing_public
            .verify(&signed_data, &signature)
            .map_err(|e| Error::InvalidState(format!("key-exchange signature invalid: {e}")))
    }

    // ---- Key exchange processing ------------------------------------

    /// Process an incoming key exchange message.
    ///
    /// 1. Verifies Ed25519 signature.
    /// 2. Checks timestamp freshness.
    /// 3. Decrypts the key via DH shared secret.
    /// 4. Verifies `epoch_fingerprint` matches.
    /// 5. For `POST_JOIN`: verifies `parent_fingerprint`, stores as candidate.
    /// 6. For `FULL_ARCHIVE`: adds to consensus collector.
    #[allow(clippy::too_many_lines, reason = "key exchange verification encompasses multiple security checks that must be atomic")]
    pub fn receive_key_exchange(
        &mut self,
        exchange: &PchatKeyExchange,
        request_timestamp: Option<u64>,
    ) -> Result<()> {
        // 1. Verify signature
        self.verify_key_exchange_signature(exchange)?;

        // 2. Timestamp freshness
        if let Some(req_ts) = request_timestamp {
            if exchange.timestamp < req_ts {
                return Err(Error::InvalidState(
                    "key-exchange timestamp before request".into(),
                ));
            }
            if exchange.timestamp > req_ts + KEY_EXCHANGE_FRESHNESS_MS {
                return Err(Error::InvalidState(
                    "key-exchange timestamp too far after request".into(),
                ));
            }
        }

        // 3. Decrypt the key via DH
        let peer = self
            .peer_keys
            .get(&exchange.sender_hash)
            .ok_or_else(|| Error::InvalidState("unknown sender".into()))?;

        let shared_secret = self.identity.dh_agree(&peer.dh_public);
        let decrypt_key = self
            .suite.key_deriver()
            .derive(&shared_secret, encryption::HKDF_SALT_IDENTITY, b"key-wrap")?;

        let decrypted_key_bytes = self
            .suite.encryptor()
            .decrypt(&decrypt_key, &exchange.encrypted_key, &[])?;

        if decrypted_key_bytes.len() != 32 {
            return Err(Error::InvalidState(format!(
                "decrypted key is {} bytes, expected 32",
                decrypted_key_bytes.len()
            )));
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&decrypted_key_bytes);

        // 4. Verify epoch_fingerprint
        let computed_fp = epoch_fingerprint(&key_bytes);
        if exchange.epoch_fingerprint.len() != 8 || computed_fp != exchange.epoch_fingerprint[..8] {
            return Err(Error::InvalidState(
                "epoch_fingerprint mismatch".into(),
            ));
        }

        let protocol = PchatProtocol::from_wire_str(&exchange.protocol);

        match protocol {
            PchatProtocol::FancyV1FullArchive => {
                // 6. Add to consensus collector
                if let Some(ref request_id) = exchange.request_id {
                    let collector =
                        self.pending_consensus
                            .entry(request_id.clone())
                            .or_insert_with(|| ConsensusCollector {
                                window_start: Instant::now(),
                                responses: HashMap::new(),
                                request_timestamp: request_timestamp.unwrap_or(0),
                                observed_members: 0,
                            });
                    let _ = collector
                        .responses
                        .insert(exchange.sender_hash.clone(), key_bytes.to_vec());
                } else {
                    // Direct key acceptance (no request_id, e.g. key custodian shortcut)
                    let _ = self.archive_keys.insert(
                        exchange.channel_id,
                        (ChannelKey { key: key_bytes }, KeyTrustLevel::Unverified),
                    );
                }
            }
            _ => {
                return Err(Error::InvalidState(format!(
                    "unexpected protocol in key-exchange: {protocol:?}"
                )));
            }
        }

        // Check for inline countersignature
        if let (Some(ref countersig), Some(ref countersigner)) =
            (&exchange.countersignature, &exchange.countersigner_hash)
        {
            let parent_fp = exchange
                .parent_fingerprint
                .as_deref()
                .unwrap_or(&[0u8; 8]);
            let _ = self.verify_countersignature_internal(
                exchange.channel_id,
                exchange.epoch,
                &exchange.epoch_fingerprint,
                parent_fp,
                countersigner,
                &exchange.sender_hash,
                exchange.timestamp,
                countersig,
            );
        }

        Ok(())
    }

    // ---- Key distribution -------------------------------------------

    /// Generate a key-exchange payload for distributing a key to a new member.
    #[allow(clippy::too_many_arguments, reason = "key distribution requires all cryptographic parameters")]
    pub fn distribute_key(
        &self,
        channel_id: u32,
        protocol: PchatProtocol,
        epoch: u32,
        recipient_hash: &str,
        recipient_public: &X25519PublicKey,
        request_id: Option<&str>,
        timestamp: u64,
    ) -> Result<PchatKeyExchange> {
        let key_bytes = match protocol {
            PchatProtocol::FancyV1FullArchive => {
                let (channel_key, _) = self
                    .archive_keys
                    .get(&channel_id)
                    .ok_or_else(|| Error::InvalidState("no archive key".into()))?;
                channel_key.key
            }
            _ => {
                return Err(Error::InvalidState(format!(
                    "cannot distribute key for protocol {protocol:?}"
                )));
            }
        };

        // Encrypt the key to the recipient's X25519 public key via DH
        let shared_secret = self.identity.dh_agree(recipient_public);
        let wrap_key = self
            .suite.key_deriver()
            .derive(&shared_secret, encryption::HKDF_SALT_IDENTITY, b"key-wrap")?;
        let encrypted_key = self.suite.encryptor().encrypt(&wrap_key, &key_bytes, &[])?;

        // Compute fingerprints
        let efp = epoch_fingerprint(&key_bytes);
        let parent_fp = None;

        // Build and sign
        let signed_data = build_key_exchange_signed_data(
            ALGORITHM_VERSION,
            channel_id,
            &protocol,
            epoch,
            &encrypted_key,
            recipient_hash,
            request_id,
            timestamp,
        );
        let signature = self.identity.sign(&signed_data);

        Ok(PchatKeyExchange {
            channel_id,
            protocol: protocol.as_wire_str().to_string(),
            epoch,
            encrypted_key,
            sender_hash: String::new(), // caller fills in cert_hash
            recipient_hash: recipient_hash.to_string(),
            request_id: request_id.map(String::from),
            timestamp,
            algorithm_version: ALGORITHM_VERSION,
            signature: signature.to_bytes().to_vec(),
            parent_fingerprint: parent_fp,
            epoch_fingerprint: efp.to_vec(),
            countersignature: None,
            countersigner_hash: None,
        })
    }

    // ---- Key request handling ---------------------------------------

    /// Handle an incoming key request. Returns a key-exchange payload
    /// if we hold the key and have not exceeded the batch limit.
    pub fn handle_key_request(
        &mut self,
        request: &PchatKeyRequest,
        our_cert_hash: &str,
    ) -> Result<Option<PchatKeyExchange>> {
        if self.requests_processed >= self.max_requests_per_connection {
            return Ok(None);
        }

        if request.requester_public.len() != 32 {
            return Err(Error::InvalidState(
                "invalid requester public key length".into(),
            ));
        }

        let protocol = PchatProtocol::from_wire_str(&request.protocol);
        let channel_id = request.channel_id;

        // Check if we hold the key for this channel
        let has_key = match protocol {
            PchatProtocol::FancyV1FullArchive => self.archive_keys.contains_key(&channel_id),
            _ => false,
        };

        if !has_key {
            return Ok(None);
        }

        let epoch = 0;

        let mut requester_key_bytes = [0u8; 32];
        requester_key_bytes.copy_from_slice(&request.requester_public);
        let recipient_public = X25519PublicKey::from(requester_key_bytes);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let mut exchange = self.distribute_key(
            channel_id,
            protocol,
            epoch,
            &request.requester_hash,
            &recipient_public,
            Some(&request.request_id),
            now,
        )?;
        exchange.sender_hash = our_cert_hash.to_string();

        self.requests_processed += 1;
        Ok(Some(exchange))
    }
}
