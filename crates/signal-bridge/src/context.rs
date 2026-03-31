//! Internal context managing Sender Key state for group encryption.
//!
//! Uses `libsignal-protocol`'s Sender Key operations and runs async
//! operations on a blocking tokio runtime.  A custom
//! [`PersistableSenderKeyStore`] replaces `InMemSenderKeyStore` so that
//! all stored records can be enumerated and serialized for persistence.

use std::collections::{HashMap, HashSet};

use libsignal_protocol::{
    DeviceId, ProtocolAddress, SenderKeyDistributionMessage, SenderKeyRecord, SenderKeyStore,
    create_sender_key_distribution_message, group_decrypt, group_encrypt,
    process_sender_key_distribution_message,
};
use rand::rngs::OsRng;
use rand::TryRngCore as _;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// SIGNAL_V1 namespace UUID for deriving deterministic distribution IDs
/// from channel IDs. Uses UUID v5 (SHA-1 based).
const NAMESPACE_SIGNAL_V1: Uuid = Uuid::from_bytes([
    0xfa, 0x6c, 0x79, 0x4d, 0x75, 0x6d, 0x62, 0x6c, 0x65, 0x53, 0x69, 0x67, 0x56, 0x31, 0x00,
    0x00,
]);

/// Device ID used for all FancyMumble users (single-device model).
fn device_id() -> DeviceId {
    DeviceId::new(1).expect("device id 1 is valid")
}

// ---------------------------------------------------------------------------
// Persistable Sender Key Store
// ---------------------------------------------------------------------------

/// A sender key store that keeps all records in a `HashMap` and supports
/// full enumeration (unlike `InMemSenderKeyStore`).
struct PersistableSenderKeyStore {
    /// Records keyed by `(address_name, distribution_id)`.
    records: HashMap<(String, Uuid), SenderKeyRecord>,
}

impl PersistableSenderKeyStore {
    fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    /// Return an iterator over all stored records for serialization.
    fn iter(&self) -> impl Iterator<Item = (&str, &Uuid, &SenderKeyRecord)> {
        self.records
            .iter()
            .map(|((addr, dist_id), record)| (addr.as_str(), dist_id, record))
    }

    /// Remove all records matching a given distribution UUID.
    fn remove_by_distribution(&mut self, dist_id: &Uuid) {
        self.records.retain(|(_, d), _| d != dist_id);
    }
}

#[async_trait::async_trait(?Send)]
impl SenderKeyStore for PersistableSenderKeyStore {
    async fn store_sender_key(
        &mut self,
        sender: &ProtocolAddress,
        distribution_id: Uuid,
        record: &SenderKeyRecord,
    ) -> Result<(), libsignal_protocol::error::SignalProtocolError> {
        self.records.insert(
            (sender.name().to_owned(), distribution_id),
            record.clone(),
        );
        Ok(())
    }

    async fn load_sender_key(
        &mut self,
        sender: &ProtocolAddress,
        distribution_id: Uuid,
    ) -> Result<Option<SenderKeyRecord>, libsignal_protocol::error::SignalProtocolError> {
        Ok(self
            .records
            .get(&(sender.name().to_owned(), distribution_id))
            .cloned())
    }
}

/// Bridge context holding all Signal Sender Key state.
pub struct SignalBridgeCtx {
    /// Our address (cert_hash based).
    our_address: ProtocolAddress,
    /// Persistable sender key store (supports enumeration for export).
    store: PersistableSenderKeyStore,
    /// Tokio runtime for running async libsignal operations.
    rt: tokio::runtime::Runtime,
    /// Channels we have created our own sender key for.
    our_channels: HashSet<u32>,
    /// Track which (sender, channel) pairs we have keys for.
    known_keys: HashSet<(String, u32)>,
    /// Cached initial SKDM bytes per distribution UUID.
    ///
    /// When we first create a sender key distribution for a channel, the
    /// SKDM contains the chain seed at iteration 0.  After encrypting
    /// messages the chain advances, so subsequent calls to
    /// `create_sender_key_distribution_message` would return the current
    /// (advanced) iteration.  Recipients who only receive that later SKDM
    /// cannot derive backward and will fail to decrypt messages encrypted
    /// at earlier iterations (e.g. offline-queued messages).
    ///
    /// By caching the initial SKDM and returning it on every re-distribution
    /// we guarantee that every recipient starts at iteration 0.
    initial_distributions: HashMap<Uuid, Vec<u8>>,
}

impl SignalBridgeCtx {
    /// Create a new context for the given address string (cert hash).
    pub fn new(our_address: String) -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime");
        Self {
            our_address: ProtocolAddress::new(our_address, device_id()),
            store: PersistableSenderKeyStore::new(),
            rt,
            our_channels: HashSet::new(),
            known_keys: HashSet::new(),
            initial_distributions: HashMap::new(),
        }
    }

    /// Derive a deterministic distribution UUID from a channel ID.
    fn distribution_id(channel_id: u32) -> Uuid {
        Uuid::new_v5(&NAMESPACE_SIGNAL_V1, &channel_id.to_le_bytes())
    }

    /// Create our sender key distribution message for a channel.
    ///
    /// Returns the serialized `SenderKeyDistributionMessage` bytes.
    /// The initial SKDM (at iteration 0) is cached so that subsequent
    /// calls return the same bytes, ensuring all recipients can decrypt
    /// messages from the very first iteration of the chain.
    pub fn create_distribution(&mut self, channel_id: u32) -> Result<Vec<u8>, String> {
        let dist_id = Self::distribution_id(channel_id);

        // Return cached initial distribution if available.
        if let Some(cached) = self.initial_distributions.get(&dist_id) {
            self.our_channels.insert(channel_id);
            return Ok(cached.clone());
        }

        let skdm = self
            .rt
            .block_on(create_sender_key_distribution_message(
                &self.our_address,
                dist_id,
                &mut self.store,
                &mut OsRng.unwrap_err(),
            ))
            .map_err(|e| format!("create_distribution: {e}"))?;
        let bytes = skdm.serialized().to_vec();

        self.initial_distributions.insert(dist_id, bytes.clone());
        self.our_channels.insert(channel_id);
        Ok(bytes)
    }

    /// Process a peer's sender key distribution message.
    ///
    /// Any existing record for this sender and distribution is removed
    /// before processing.  This prevents libsignal's
    /// `add_sender_key_state` from keeping a stale chain position from a
    /// previous session (which would cause `DuplicatedMessage` errors
    /// when the message iteration is lower than the stored chain
    /// position).
    pub fn process_distribution(
        &mut self,
        sender_hash: &str,
        channel_id: u32,
        distribution_bytes: &[u8],
    ) -> Result<(), String> {
        let sender_addr = ProtocolAddress::new(sender_hash.to_owned(), device_id());
        let skdm = SenderKeyDistributionMessage::try_from(distribution_bytes)
            .map_err(|e| format!("parse distribution: {e}"))?;

        // Clear any existing record so the new distribution is always
        // processed from scratch.  Without this, `add_sender_key_state`
        // silently keeps old state when it sees the same (chain_id,
        // signing_key), which can leave the receiver at a higher chain
        // iteration than the sender's new messages.
        let dist_id = skdm
            .distribution_id()
            .map_err(|e| format!("skdm distribution_id: {e}"))?;
        self.store
            .records
            .remove(&(sender_addr.name().to_owned(), dist_id));

        self.rt
            .block_on(process_sender_key_distribution_message(
                &sender_addr,
                &skdm,
                &mut self.store,
            ))
            .map_err(|e| format!("process_distribution: {e}"))?;
        self.known_keys
            .insert((sender_hash.to_owned(), channel_id));
        Ok(())
    }

    /// Encrypt plaintext using our sender key for a channel.
    pub fn group_encrypt(&mut self, channel_id: u32, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let dist_id = Self::distribution_id(channel_id);
        let skm = self
            .rt
            .block_on(group_encrypt(
                &mut self.store,
                &self.our_address,
                dist_id,
                plaintext,
                &mut OsRng.unwrap_err(),
            ))
            .map_err(|e| format!("group_encrypt: {e}"))?;
        Ok(skm.serialized().to_vec())
    }

    /// Decrypt ciphertext from a peer on a channel.
    pub fn group_decrypt(
        &mut self,
        sender_hash: &str,
        _channel_id: u32,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, String> {
        let sender_addr = ProtocolAddress::new(sender_hash.to_owned(), device_id());
        self.rt
            .block_on(group_decrypt(ciphertext, &mut self.store, &sender_addr))
            .map_err(|e| format!("group_decrypt: {e}"))
    }

    /// Check if we have a sender key for a peer on a channel.
    pub fn has_key(&self, sender_hash: &str, channel_id: u32) -> bool {
        self.known_keys
            .contains(&(sender_hash.to_owned(), channel_id))
    }

    /// Remove all state for a channel.
    pub fn remove_channel(&mut self, channel_id: u32) {
        let dist_id = Self::distribution_id(channel_id);
        self.our_channels.remove(&channel_id);
        self.known_keys.retain(|(_s, ch)| *ch != channel_id);
        self.initial_distributions.remove(&dist_id);
        self.store.remove_by_distribution(&dist_id);
    }

    /// Export all state as a JSON blob.
    pub fn export_state(&self) -> Result<Vec<u8>, String> {
        let entries: Vec<ExportedSenderKeyRecord> = self
            .store
            .iter()
            .map(|(sender_hash, dist_id, record)| {
                let record_bytes = record.serialize().map_err(|e| format!("serialize record: {e}"))?;
                Ok(ExportedSenderKeyRecord {
                    sender_hash: sender_hash.to_owned(),
                    distribution_id: dist_id.to_string(),
                    record_bytes,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let state = ExportedState {
            our_address: self.our_address.name().to_owned(),
            our_channels: self.our_channels.iter().copied().collect(),
            known_keys: self
                .known_keys
                .iter()
                .map(|(s, ch)| ExportedKeyEntry {
                    sender_hash: s.clone(),
                    channel_id: *ch,
                })
                .collect(),
            sender_key_records: entries,
            initial_distributions: self
                .initial_distributions
                .iter()
                .map(|(dist_id, skdm_bytes)| ExportedInitialDistribution {
                    distribution_id: dist_id.to_string(),
                    skdm_bytes: skdm_bytes.clone(),
                })
                .collect(),
        };
        serde_json::to_vec(&state).map_err(|e| format!("export: {e}"))
    }

    /// Import state from a JSON blob.
    pub fn import_state(&mut self, data: &[u8]) -> Result<(), String> {
        let state: ExportedState =
            serde_json::from_slice(data).map_err(|e| format!("import: {e}"))?;

        self.our_channels = state.our_channels.into_iter().collect();
        self.known_keys = state
            .known_keys
            .into_iter()
            .map(|e| (e.sender_hash, e.channel_id))
            .collect();

        // Restore sender key records into the store.
        for entry in state.sender_key_records {
            let addr = ProtocolAddress::new(entry.sender_hash, device_id());
            let dist_id = Uuid::parse_str(&entry.distribution_id)
                .map_err(|e| format!("bad uuid: {e}"))?;
            let record = SenderKeyRecord::deserialize(&entry.record_bytes)
                .map_err(|e| format!("bad record: {e}"))?;
            self.rt
                .block_on(self.store.store_sender_key(&addr, dist_id, &record))
                .map_err(|e| format!("store import: {e}"))?;
        }

        // Restore cached initial distributions.
        for entry in state.initial_distributions {
            let dist_id = Uuid::parse_str(&entry.distribution_id)
                .map_err(|e| format!("bad initial dist uuid: {e}"))?;
            self.initial_distributions.insert(dist_id, entry.skdm_bytes);
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Serialization types
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct ExportedState {
    our_address: String,
    our_channels: Vec<u32>,
    known_keys: Vec<ExportedKeyEntry>,
    sender_key_records: Vec<ExportedSenderKeyRecord>,
    /// Cached initial SKDM bytes per distribution UUID.
    /// Uses `serde(default)` for backward compatibility with old state files.
    #[serde(default)]
    initial_distributions: Vec<ExportedInitialDistribution>,
}

#[derive(Serialize, Deserialize)]
struct ExportedKeyEntry {
    sender_hash: String,
    channel_id: u32,
}

#[derive(Serialize, Deserialize)]
struct ExportedSenderKeyRecord {
    sender_hash: String,
    distribution_id: String,
    #[serde(with = "base64_bytes")]
    record_bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct ExportedInitialDistribution {
    distribution_id: String,
    #[serde(with = "base64_bytes")]
    skdm_bytes: Vec<u8>,
}

mod base64_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;
        STANDARD.encode(bytes).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;
        let s = String::deserialize(d)?;
        STANDARD.decode(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    fn make_ctx(addr: &str) -> SignalBridgeCtx {
        SignalBridgeCtx::new(addr.to_owned())
    }

    /// Verify that `create_distribution` returns identical bytes on
    /// repeated calls (initial SKDM is cached at iteration 0).
    #[test]
    fn create_distribution_returns_cached_bytes() {
        let mut ctx = make_ctx("alice");
        let first = ctx.create_distribution(10).unwrap();
        let second = ctx.create_distribution(10).unwrap();
        assert_eq!(first, second, "second call must return cached initial SKDM");
    }

    /// Verify that the cached initial SKDM is still returned even after
    /// the sender encrypts messages (which advances the chain).
    #[test]
    fn create_distribution_stable_after_encrypt() {
        let mut ctx = make_ctx("alice");
        let initial = ctx.create_distribution(10).unwrap();

        // Encrypt a few messages to advance the chain.
        ctx.group_encrypt(10, b"msg1").unwrap();
        ctx.group_encrypt(10, b"msg2").unwrap();

        let after_encrypt = ctx.create_distribution(10).unwrap();
        assert_eq!(
            initial, after_encrypt,
            "distribution must remain the initial SKDM even after chain advances"
        );
    }

    /// Verify that a recipient can decrypt a message using only the
    /// initial SKDM, even after the sender has encrypted prior messages
    /// and re-distributed.
    ///
    /// This is the core scenario that caused the offline decrypt bug:
    /// the sender encrypts a message (advancing the chain), then
    /// re-distributes.  If the distribution contained the advanced
    /// chain state, a fresh recipient couldn't decrypt the earlier
    /// message.
    #[test]
    fn recipient_decrypts_with_initial_distribution_after_chain_advance() {
        let mut sender = make_ctx("sender_hash");
        let mut recipient = make_ctx("recipient_hash");

        // Sender creates initial distribution for channel 10.
        let dist = sender.create_distribution(10).unwrap();

        // Sender encrypts a message (chain advances).
        let ct = sender.group_encrypt(10, b"hello world").unwrap();

        // Re-distribute: should still return the initial SKDM.
        let redist = sender.create_distribution(10).unwrap();
        assert_eq!(dist, redist);

        // Recipient processes only the re-distributed SKDM.
        recipient
            .process_distribution("sender_hash", 10, &redist)
            .unwrap();

        // Recipient must be able to decrypt the message even though it
        // was encrypted at iteration 0 and the chain has since advanced.
        let pt = recipient.group_decrypt("sender_hash", 10, &ct).unwrap();
        assert_eq!(pt, b"hello world");
    }

    /// Verify that `remove_channel` clears the cached SKDM so the next
    /// `create_distribution` generates a fresh chain.
    #[test]
    fn remove_channel_clears_cached_distribution() {
        let mut ctx = make_ctx("alice");
        let first = ctx.create_distribution(10).unwrap();
        ctx.remove_channel(10);
        let second = ctx.create_distribution(10).unwrap();
        // After removing, a brand-new chain is created, so the bytes
        // should differ (different random chain_id).
        assert_ne!(
            first, second,
            "after remove_channel, a new chain must be created"
        );
    }

    /// Verify that export/import round-trips the cached initial
    /// distributions correctly.
    #[test]
    fn export_import_preserves_initial_distributions() {
        let mut ctx = make_ctx("alice");
        let initial = ctx.create_distribution(10).unwrap();

        // Advance the chain so the current state differs from the cached
        // initial distribution.
        ctx.group_encrypt(10, b"advance").unwrap();

        let exported = ctx.export_state().unwrap();

        // Create a fresh context and import the state.
        let mut ctx2 = make_ctx("alice");
        ctx2.import_state(&exported).unwrap();

        // The imported context must return the same initial distribution.
        let imported_dist = ctx2.create_distribution(10).unwrap();
        assert_eq!(
            initial, imported_dist,
            "imported state must return the same initial distribution"
        );
    }

    /// Verify backward compatibility: importing old state (without
    /// `initial_distributions` field) works without errors.
    #[test]
    fn import_old_state_without_initial_distributions() {
        // Simulate old state format without `initial_distributions`.
        let old_json = serde_json::json!({
            "our_address": "alice",
            "our_channels": [],
            "known_keys": [],
            "sender_key_records": []
        });
        let data = serde_json::to_vec(&old_json).unwrap();

        let mut ctx = make_ctx("alice");
        ctx.import_state(&data).unwrap();

        // Should still work: creates a fresh distribution.
        let dist = ctx.create_distribution(10).unwrap();
        assert!(!dist.is_empty());
    }

    /// Verify that `process_distribution` clears stale state so that
    /// a restored receiver can decrypt messages after a fresh
    /// distribution from the sender.
    ///
    /// Scenario: receiver previously consumed messages (advancing the
    /// stored chain), and the sender re-distributes at a lower iteration
    /// than the receiver's stored chain position.  Without the clear,
    /// `add_sender_key_state` keeps the advanced state and future
    /// messages fail with `DuplicatedMessage`.
    #[test]
    fn process_distribution_clears_stale_state_on_receiver() {
        let mut sender = make_ctx("sender_hash");
        let mut receiver = make_ctx("receiver_hash");

        // 1. Sender creates distribution and encrypts 5 messages.
        let dist = sender.create_distribution(10).unwrap();
        let mut ciphertexts = Vec::new();
        for i in 0..5 {
            ciphertexts.push(
                sender
                    .group_encrypt(10, format!("msg {i}").as_bytes())
                    .unwrap(),
            );
        }

        // 2. Receiver processes the distribution and decrypts all 5.
        receiver
            .process_distribution("sender_hash", 10, &dist)
            .unwrap();
        for (i, ct) in ciphertexts.iter().enumerate() {
            let pt = receiver.group_decrypt("sender_hash", 10, ct).unwrap();
            assert_eq!(pt, format!("msg {i}").as_bytes());
        }

        // 3. Receiver's chain is now at iteration 5.  Simulate a
        //    reconnect: export, create fresh receiver, import.
        let exported = receiver.export_state().unwrap();
        let mut receiver2 = make_ctx("receiver_hash");
        receiver2.import_state(&exported).unwrap();

        // 4. Sender creates a fresh distribution (cached at iter=0).
        //    This is what happens when sender sends SKDM to a new user.
        let redist = sender.create_distribution(10).unwrap();

        // 5. Receiver processes the fresh distribution.  The clear in
        //    process_distribution must reset the chain to iter=0.
        receiver2
            .process_distribution("sender_hash", 10, &redist)
            .unwrap();

        // 6. Sender encrypts a new message (at iteration 5 on sender's
        //    chain which has advanced past the cached SKDM).
        let new_ct = sender.group_encrypt(10, b"after reconnect").unwrap();

        // 7. Receiver must be able to decrypt it by deriving forward
        //    from iter=0 (not stuck at iter=5 from old state).
        let pt = receiver2
            .group_decrypt("sender_hash", 10, &new_ct)
            .unwrap();
        assert_eq!(pt, b"after reconnect");
    }
}
