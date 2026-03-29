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
        }
    }

    /// Derive a deterministic distribution UUID from a channel ID.
    fn distribution_id(channel_id: u32) -> Uuid {
        Uuid::new_v5(&NAMESPACE_SIGNAL_V1, &channel_id.to_le_bytes())
    }

    /// Create our sender key distribution message for a channel.
    ///
    /// Returns the serialized `SenderKeyDistributionMessage` bytes.
    pub fn create_distribution(&mut self, channel_id: u32) -> Result<Vec<u8>, String> {
        let dist_id = Self::distribution_id(channel_id);
        let skdm = self
            .rt
            .block_on(create_sender_key_distribution_message(
                &self.our_address,
                dist_id,
                &mut self.store,
                &mut OsRng.unwrap_err(),
            ))
            .map_err(|e| format!("create_distribution: {e}"))?;
        self.our_channels.insert(channel_id);
        Ok(skdm.serialized().to_vec())
    }

    /// Process a peer's sender key distribution message.
    pub fn process_distribution(
        &mut self,
        sender_hash: &str,
        channel_id: u32,
        distribution_bytes: &[u8],
    ) -> Result<(), String> {
        let sender_addr = ProtocolAddress::new(sender_hash.to_owned(), device_id());
        let skdm = SenderKeyDistributionMessage::try_from(distribution_bytes)
            .map_err(|e| format!("parse distribution: {e}"))?;
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
        self.our_channels.remove(&channel_id);
        self.known_keys.retain(|(_s, ch)| *ch != channel_id);
        // Note: InMemSenderKeyStore does not provide a remove API,
        // so old sender key records remain until the context is destroyed.
        // This is acceptable because they cannot decrypt new messages
        // after the channel's distribution ID changes (new epoch).
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
