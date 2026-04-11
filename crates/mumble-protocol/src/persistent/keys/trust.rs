//! Trust authority checks, custodian TOFU management, countersignature
//! verification, dispute resolution, and trust level queries.

use std::collections::HashSet;

use ed25519_dalek::Verifier;

use crate::error::{Error, Result};
use crate::persistent::encryption::build_countersig_data;
use crate::persistent::{KeyTrustLevel, PchatProtocol, StoredMessage};

use super::types::{CustodianPinState, EncryptedPayload, COUNTERSIG_FRESHNESS_MS};
use super::KeyManager;

impl KeyManager {
    // ---- Trust authority checks -------------------------------------

    /// Check if a sender is a trusted authority for a channel.
    ///
    /// Returns true only when all conditions from section 5.7 are met:
    /// 1. Sender appears in `key_custodians` or is the channel originator.
    /// 2. Sender appears in the TOFU-pinned list.
    /// 3. The pinned list has been confirmed by the user.
    pub fn is_trusted_authority(
        &self,
        sender_hash: &str,
        channel_id: u32,
        key_custodians: &[String],
    ) -> bool {
        self.is_trusted_authority_internal(sender_hash, channel_id, key_custodians)
    }

    pub(super) fn is_trusted_authority_internal(
        &self,
        sender_hash: &str,
        channel_id: u32,
        key_custodians: &[String],
    ) -> bool {
        // Check 1: sender in custodians or is channel originator
        let in_server_list = key_custodians.iter().any(|h| h == sender_hash);
        let is_originator = self
            .channel_originators
            .get(&channel_id)
            .is_some_and(|h| h == sender_hash);

        if !in_server_list && !is_originator {
            return false;
        }

        // Check 2 & 3: sender in confirmed pinned list
        if let Some(pin_state) = self.pinned_custodians.get(&channel_id) {
            if !pin_state.confirmed {
                return false;
            }
            pin_state.pinned.iter().any(|h| h == sender_hash) || is_originator
        } else {
            // No pinned state yet - only originator is trusted
            is_originator
        }
    }

    // ---- Countersignature verification ------------------------------

    /// Verify an epoch countersignature (standalone or inline).
    #[allow(clippy::too_many_arguments, reason = "countersignature verification requires all cryptographic parameters")]
    pub fn verify_countersignature(
        &mut self,
        channel_id: u32,
        epoch: u32,
        epoch_fp: &[u8],
        parent_fp: &[u8],
        signer_hash: &str,
        distributor_hash: &str,
        timestamp: u64,
        countersignature: &[u8],
        key_custodians: &[String],
    ) -> Result<KeyTrustLevel> {
        self.verify_countersignature_internal(
            channel_id,
            epoch,
            epoch_fp,
            parent_fp,
            signer_hash,
            distributor_hash,
            timestamp,
            countersignature,
        )?;

        // Verify signer is a trusted authority
        if !self.is_trusted_authority_internal(signer_hash, channel_id, key_custodians) {
            return Err(Error::InvalidState(
                "countersigner is not a trusted authority".into(),
            ));
        }

        // Promote epoch key to Verified
        if let Some(epochs) = self.epoch_keys.get_mut(&channel_id) {
            if let Some((_key, trust)) = epochs.get_mut(&epoch) {
                *trust = KeyTrustLevel::Verified;
            }
        }

        Ok(KeyTrustLevel::Verified)
    }

    #[allow(clippy::too_many_arguments, reason = "internal verification helper requires all cryptographic parameters")]
    pub(super) fn verify_countersignature_internal(
        &self,
        channel_id: u32,
        epoch: u32,
        epoch_fp: &[u8],
        parent_fp: &[u8],
        signer_hash: &str,
        distributor_hash: &str,
        timestamp: u64,
        countersignature: &[u8],
    ) -> Result<()> {
        // Timestamp freshness
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        if now > timestamp + COUNTERSIG_FRESHNESS_MS {
            return Err(Error::InvalidState(
                "countersignature timestamp too old".into(),
            ));
        }

        // Verify Ed25519 signature
        let peer = self.peer_keys.get(signer_hash).ok_or_else(|| {
            Error::InvalidState(format!("unknown countersigner: {signer_hash}"))
        })?;

        let data = build_countersig_data(
            channel_id,
            epoch,
            epoch_fp,
            parent_fp,
            timestamp,
            distributor_hash,
        );

        let sig = ed25519_dalek::Signature::from_slice(countersignature)
            .map_err(|e| Error::InvalidState(format!("invalid countersig bytes: {e}")))?;

        peer.signing_public
            .verify(&data, &sig)
            .map_err(|e| Error::InvalidState(format!("countersignature invalid: {e}")))
    }

    // ---- Custodian TOFU management ----------------------------------

    /// Update the pinned custodian list from a `ChannelState` update.
    ///
    /// Returns `true` if the list changed and needs user acceptance.
    pub fn update_custodian_pin(&mut self, channel_id: u32, new_custodians: Vec<String>) -> bool {
        if let Some(pin_state) = self.pinned_custodians.get_mut(&channel_id) {
            if pin_state.pinned == new_custodians {
                return false; // no change
            }
            pin_state.pending_update = Some(new_custodians);
            true
        } else {
            // First observation
            let _ = self.pinned_custodians.insert(
                channel_id,
                CustodianPinState::first_observation(new_custodians),
            );
            // Needs confirmation if list is non-empty
            self.pinned_custodians
                .get(&channel_id)
                .is_some_and(|s| !s.confirmed)
        }
    }

    /// Accept a pending custodian list update (user clicked "Accept").
    pub fn accept_custodian_update(&mut self, channel_id: u32) {
        if let Some(pin_state) = self.pinned_custodians.get_mut(&channel_id) {
            if let Some(new_list) = pin_state.pending_update.take() {
                pin_state.pinned = new_list;
            }
            pin_state.confirmed = true;
        }
    }

    /// Confirm the initial custodian list (user clicked "Confirm" on first join).
    pub fn confirm_custodian_list(&mut self, channel_id: u32) {
        if let Some(pin_state) = self.pinned_custodians.get_mut(&channel_id) {
            pin_state.confirmed = true;
        }
    }

    /// Get the current custodian pin state for a channel.
    pub fn get_custodian_pin(&self, channel_id: u32) -> Option<&CustodianPinState> {
        self.pinned_custodians.get(&channel_id)
    }

    // ---- Key trial decryption (supplementary check) -----------------

    /// Attempt to verify a key by decrypting recent messages.
    ///
    /// Returns true if decryption succeeds for messages from 2+ distinct
    /// senders. This is a diagnostic signal only and does NOT promote
    /// trust level.
    pub fn check_key_by_decryption(
        &self,
        channel_id: u32,
        mode: PchatProtocol,
        messages: &[StoredMessage],
    ) -> bool {
        let mut successful_senders = HashSet::new();

        for msg in messages {
            if !msg.encrypted {
                continue;
            }
            let payload = EncryptedPayload {
                ciphertext: msg.body.as_bytes().to_vec(),
                epoch: msg.epoch,
                chain_index: msg.chain_index,
                epoch_fingerprint: [0; 8], // not checked here
            };
            if self
                .decrypt(mode, channel_id, &msg.message_id, msg.timestamp, &payload)
                .is_ok()
            {
                let _ = successful_senders.insert(&msg.sender_hash);
            }
        }

        successful_senders.len() >= 2
    }

    // ---- Dispute resolution -----------------------------------------

    /// Resolve a dispute by manually selecting a trusted peer's key.
    pub fn resolve_dispute(
        &mut self,
        channel_id: u32,
        mode: PchatProtocol,
        _trusted_sender_hash: &str,
    ) -> Result<()> {
        match mode {
            PchatProtocol::FancyV1FullArchive => {
                if let Some((_key, trust)) = self.archive_keys.get_mut(&channel_id) {
                    *trust = KeyTrustLevel::ManuallyVerified;
                }
                Ok(())
            }
            _ => Err(Error::InvalidState(format!(
                "cannot resolve dispute for mode {mode:?}"
            ))),
        }
    }

    // ---- Trust level query ------------------------------------------

    /// Get the trust level for a channel's current key.
    pub fn trust_level(&self, channel_id: u32, mode: PchatProtocol) -> Option<KeyTrustLevel> {
        match mode {
            PchatProtocol::FancyV1FullArchive => {
                self.archive_keys.get(&channel_id).map(|(_, trust)| *trust)
            }
            _ => None,
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
    fn trust_level_query() {
        let mut km = make_key_manager();
        assert!(km.trust_level(1, PchatProtocol::FancyV1FullArchive).is_none());

        km.store_archive_key(1, [0; 32], KeyTrustLevel::Unverified);
        assert_eq!(
            km.trust_level(1, PchatProtocol::FancyV1FullArchive),
            Some(KeyTrustLevel::Unverified)
        );

        km.store_archive_key(2, [0; 32], KeyTrustLevel::Verified);
        assert_eq!(
            km.trust_level(2, PchatProtocol::FancyV1FullArchive),
            Some(KeyTrustLevel::Verified)
        );
    }

    #[test]
    fn trusted_authority_requires_confirmation() {
        let mut km = make_key_manager();
        let custodians = vec!["alice_hash".to_string()];

        // Pin without confirmation
        let _ = km.update_custodian_pin(1, custodians.clone());
        assert!(!km.is_trusted_authority("alice_hash", 1, &custodians));

        // Confirm
        km.confirm_custodian_list(1);
        assert!(km.is_trusted_authority("alice_hash", 1, &custodians));
    }

    #[test]
    fn channel_originator_is_trusted() {
        let mut km = make_key_manager();
        km.set_channel_originator(1, "bob_hash".into());
        // Originator trusted even without custodian list
        assert!(km.is_trusted_authority("bob_hash", 1, &[]));
    }
}
