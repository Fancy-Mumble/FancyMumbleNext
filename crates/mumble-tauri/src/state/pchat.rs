//! Persistent encrypted chat integration layer.
//!
//! Bridges `mumble-protocol`'s persistent chat primitives (`KeyManager`,
//! wire structs, encryption) to the Tauri application state. Handles
//! sending and receiving pchat messages using native protobuf message types.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{debug, info, warn};

use fancy_utils::hex::{bytes_to_hex, hex_decode};
use mumble_protocol::client::ClientHandle;
use mumble_protocol::command;
use mumble_protocol::persistent::keys::{KeyManager, SeedIdentity};
use mumble_protocol::persistent::protocol::signal_v1::SignalBridge;
use mumble_protocol::persistent::provider::{
    CompositeMessageProvider, InMemoryPersistentBackend, PersistentMessageProvider,
    VolatileMessageProvider,
};
use mumble_protocol::persistent::wire::{
    MsgPackCodec, MessageEnvelope,
    PchatKeyAnnounce as WireKeyAnnounce,
    PchatKeyExchange as WireKeyExchange,
    PchatKeyRequest as WireKeyRequest,
    WireCodec,
};
use mumble_protocol::persistent::PchatProtocol;
use mumble_protocol::proto::mumble_tcp;

use super::local_cache::{CachedMessage, LocalMessageCache};
use super::types::ChatMessage;
use super::SharedState;

/// Persistent chat manager -- lives inside `SharedState`.
#[allow(dead_code, reason = "pchat feature is under development; fields will be used when pchat commands are wired up")]
pub(crate) struct PchatState {
    /// Our E2EE key manager (identity + peer keys + epoch/archive keys).
    pub key_manager: KeyManager,
    /// Our TLS certificate hash (stable identity across sessions).
    pub own_cert_hash: String,
    /// `MessagePack` codec for wire serialization.
    pub codec: MsgPackCodec,
    /// Identity seed bytes (persisted to disk).
    pub seed: [u8; 32],
    /// Message provider (composite: volatile + persistent).
    pub provider: CompositeMessageProvider,
    /// Channels where we've already sent a fetch request (avoid duplicates).
    pub fetched_channels: std::collections::HashSet<u32>,
    /// Path to the per-identity storage directory (for persisting archive keys).
    pub identity_dir: Option<PathBuf>,
    /// Signal Protocol bridge (loaded from external DLL, AGPL-isolated).
    pub signal_bridge: Option<Arc<SignalBridge>>,
    /// Encrypted local message cache for `SignalV1` channels.
    /// Stores decrypted plaintext on disk with AES-256-GCM encryption.
    pub local_cache: Option<LocalMessageCache>,
    /// Stashed encrypted envelopes for `SignalV1` messages that arrived before
    /// the sender's distribution key.  Keyed by `(channel_id, sender_hash)`.
    /// Retried when we process the corresponding sender key distribution.
    pub pending_signal_envelopes: Vec<PendingSignalEnvelope>,
}

/// A `SignalV1` encrypted envelope that could not be decrypted because the
/// sender's distribution key had not yet arrived.
#[derive(Clone)]
pub(crate) struct PendingSignalEnvelope {
    pub message_id: String,
    pub channel_id: u32,
    pub timestamp: u64,
    pub sender_hash: String,
    pub envelope_bytes: Vec<u8>,
}

impl PchatState {
    /// Create a new pchat state from a 32-byte identity seed and our cert hash.
    pub fn new(
        seed: [u8; 32],
        own_cert_hash: String,
        identity_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let identity = SeedIdentity::from_seed(&seed)
            .map_err(|e| format!("Failed to derive pchat identity: {e}"))?;
        let key_manager = KeyManager::new(Box::new(identity));
        let codec = MsgPackCodec;
        let volatile = VolatileMessageProvider::new();
        let backend = InMemoryPersistentBackend::new();
        let persistent = PersistentMessageProvider::new(Box::new(backend));
        let provider = CompositeMessageProvider::new(volatile, persistent);

        let local_cache = match identity_dir.as_ref() {
            Some(dir) => match LocalMessageCache::new(dir, &seed) {
                Ok(cache) => Some(cache),
                Err(e) => {
                    warn!("failed to create local message cache: {e}");
                    None
                }
            },
            None => None,
        };

        Ok(Self {
            key_manager,
            own_cert_hash,
            codec,
            seed,
            provider,
            fetched_channels: std::collections::HashSet::new(),
            identity_dir,
            signal_bridge: None,
            local_cache,
            pending_signal_envelopes: Vec::new(),
        })
    }
}

// ---- Identity storage (per-identity: TLS cert + pchat seed) ---------

/// Top-level directory for per-identity storage.
const IDENTITIES_DIR: &str = "identities";
/// File name for the pchat identity seed inside each identity folder.
const SEED_FILE: &str = "pchat_seed.bin";
/// File name for the TLS client certificate inside each identity folder.
const TLS_CERT_FILE: &str = "tls.cert.pem";
/// File name for the TLS private key inside each identity folder.
const TLS_KEY_FILE: &str = "tls.key.pem";
/// File name for the signal bridge sender key state.
const SIGNAL_STATE_FILE: &str = "signal_state.json";

/// Legacy paths used before per-identity storage was introduced.
const LEGACY_PCHAT_DIR: &str = "pchat";
const LEGACY_SEED_FILE: &str = "identity_seed.bin";
const LEGACY_CERTS_DIR: &str = "certs";

/// Return the directory for a given identity label:
/// `<app_data>/identities/<label>/`
pub(crate) fn identity_dir(app_data_dir: &Path, label: &str) -> PathBuf {
    app_data_dir.join(IDENTITIES_DIR).join(label)
}

/// Migrate legacy storage layout to per-identity folders.
///
/// Old layout:
/// ```text
/// {app_data}/certs/{label}.cert.pem
/// {app_data}/certs/{label}.key.pem
/// {app_data}/pchat/identity_seed.bin     (single global seed)
/// ```
///
/// New layout:
/// ```text
/// {app_data}/identities/{label}/tls.cert.pem
/// {app_data}/identities/{label}/tls.key.pem
/// {app_data}/identities/{label}/pchat_seed.bin
/// ```
pub(crate) fn migrate_legacy_storage(app_data_dir: &Path) {
    let legacy_certs = app_data_dir.join(LEGACY_CERTS_DIR);
    if !legacy_certs.exists() {
        return; // nothing to migrate
    }

    // Read the global seed (may not exist).
    let global_seed_path = app_data_dir.join(LEGACY_PCHAT_DIR).join(LEGACY_SEED_FILE);
    let global_seed: Option<[u8; 32]> = std::fs::read(&global_seed_path).ok().and_then(|data| {
        if data.len() == 32 {
            let mut s = [0u8; 32];
            s.copy_from_slice(&data);
            Some(s)
        } else {
            None
        }
    });

    // Enumerate all *.cert.pem in the legacy certs directory.
    let Ok(entries) = std::fs::read_dir(&legacy_certs) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(label) = name.strip_suffix(".cert.pem") else { continue };
        let new_dir = identity_dir(app_data_dir, label);
        if new_dir.exists() {
            continue; // already migrated
        }
        if std::fs::create_dir_all(&new_dir).is_err() {
            continue;
        }

        // Move TLS cert + key.
        let old_cert = legacy_certs.join(format!("{label}.cert.pem"));
        let old_key = legacy_certs.join(format!("{label}.key.pem"));
        let _ = std::fs::copy(&old_cert, new_dir.join(TLS_CERT_FILE));
        let _ = std::fs::copy(&old_key, new_dir.join(TLS_KEY_FILE));

        // Copy the global seed into this identity’s folder.
        // (The first migrated identity inherits the existing seed so
        // archive keys derived from the old seed still work.)
        if let Some(seed) = global_seed {
            let _ = std::fs::write(new_dir.join(SEED_FILE), seed);
        }

        info!(label, "migrated legacy identity to per-identity storage");
    }

    // Remove legacy directories now that migration is complete.
    let _ = std::fs::remove_dir_all(&legacy_certs);
    let pchat_dir = app_data_dir.join(LEGACY_PCHAT_DIR);
    if pchat_dir.exists() {
        let _ = std::fs::remove_dir_all(&pchat_dir);
    }
}

/// Load or generate the 32-byte identity seed for a specific identity.
///
/// Stored in `<app_data>/identities/<label>/pchat_seed.bin`.
/// If the file doesn't exist, a new seed is generated from the OS CSPRNG.
pub(crate) fn load_or_generate_seed(
    app_data_dir: &Path,
    label: &str,
) -> Result<[u8; 32], String> {
    let dir = identity_dir(app_data_dir, label);
    let seed_path = dir.join(SEED_FILE);

    if seed_path.exists() {
        let data = std::fs::read(&seed_path).map_err(|e| format!("Failed to read seed: {e}"))?;
        if data.len() == 32 {
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&data);
            info!(label, "loaded existing pchat identity seed");
            return Ok(seed);
        }
        warn!(label, len = data.len(), "seed file has wrong length, regenerating");
    }

    // Generate new seed
    let seed: [u8; 32] = rand::random();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create identity dir: {e}"))?;
    std::fs::write(&seed_path, seed).map_err(|e| format!("Failed to write seed: {e}"))?;
    info!(label, "generated new pchat identity seed");
    Ok(seed)
}

/// Generate a self-signed TLS client certificate for an identity label.
/// Does nothing if the cert already exists.
pub(crate) fn generate_identity_cert(
    app_data_dir: &Path,
    label: &str,
) -> Result<(), String> {
    let dir = identity_dir(app_data_dir, label);
    let cert_path = dir.join(TLS_CERT_FILE);
    if cert_path.exists() {
        return Ok(()); // already exists
    }

    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create identity dir: {e}"))?;

    let certified = rcgen::generate_simple_self_signed(vec![label.to_string()])
        .map_err(|e| e.to_string())?;
    let cert_pem = certified.cert.pem();
    let key_pem = certified.signing_key.serialize_pem();

    std::fs::write(&cert_path, cert_pem).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(TLS_KEY_FILE), key_pem).map_err(|e| e.to_string())?;

    info!(label, "generated new TLS client certificate");
    Ok(())
}

/// Load TLS client certificate PEM bytes for an identity label.
/// Returns `(cert_pem, key_pem)` or `(None, None)` if not found.
pub(crate) fn load_identity_cert(
    app_data_dir: &Path,
    label: &str,
) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let dir = identity_dir(app_data_dir, label);
    let cert = std::fs::read(dir.join(TLS_CERT_FILE)).ok();
    let key = std::fs::read(dir.join(TLS_KEY_FILE)).ok();
    (cert, key)
}

/// List all identity labels (subdirectories of `identities/`).
pub(crate) fn list_identity_labels(app_data_dir: &Path) -> Vec<String> {
    let dir = app_data_dir.join(IDENTITIES_DIR);
    if !dir.exists() {
        return vec![];
    }
    let mut labels = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                // Only list directories that have a TLS cert.
                if entry.path().join(TLS_CERT_FILE).exists() {
                    labels.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }
    }
    labels.sort();
    labels
}

/// Delete an identity (TLS cert + pchat seed).
pub(crate) fn delete_identity(app_data_dir: &Path, label: &str) -> Result<(), String> {
    let dir = identity_dir(app_data_dir, label);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Export an identity to a JSON bundle at the given `dest` path.
///
/// The bundle is a JSON object with `_label`, PEM text fields, and the
/// pchat seed as a hex string.
pub(crate) fn export_identity(
    app_data_dir: &Path,
    label: &str,
    dest: &Path,
) -> Result<(), String> {
    use serde_json::{json, Map, Value};

    let dir = identity_dir(app_data_dir, label);
    if !dir.exists() {
        return Err(format!("Identity '{label}' not found"));
    }

    let mut bundle = Map::new();
    let _ = bundle.insert("_label".to_string(), Value::String(label.to_string()));

    // PEM files are UTF-8 text - store directly.
    for name in [TLS_CERT_FILE, TLS_KEY_FILE] {
        let path = dir.join(name);
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read {name}: {e}"))?;
            let _ = bundle.insert(name.to_string(), Value::String(text));
        }
    }

    // Seed is binary - hex-encode it.
    let seed_path = dir.join(SEED_FILE);
    if seed_path.exists() {
        let data = std::fs::read(&seed_path)
            .map_err(|e| format!("Failed to read seed: {e}"))?;
        let hex: String = bytes_to_hex(&data);
        let _ = bundle.insert(SEED_FILE.to_string(), Value::String(hex));
    }

    let json = serde_json::to_string_pretty(&json!(bundle))
        .map_err(|e| format!("Serialisation error: {e}"))?;
    std::fs::write(dest, json).map_err(|e| format!("Failed to write export file: {e}"))?;
    info!(label, ?dest, "exported identity");
    Ok(())
}

/// Import an identity from a JSON bundle at `src`.
///
/// Returns the label embedded in the bundle.
pub(crate) fn import_identity(
    app_data_dir: &Path,
    src: &Path,
) -> Result<String, String> {
    use serde_json::Value;

    let json = std::fs::read_to_string(src)
        .map_err(|e| format!("Failed to read import file: {e}"))?;
    let bundle: serde_json::Map<String, Value> = serde_json::from_str(&json)
        .map_err(|e| format!("Invalid identity file: {e}"))?;

    let label = bundle
        .get("_label")
        .and_then(Value::as_str)
        .ok_or("Missing _label in identity file")?
        .to_string();

    let dir = identity_dir(app_data_dir, &label);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create identity dir: {e}"))?;

    // PEM files are text - write directly.
    for name in [TLS_CERT_FILE, TLS_KEY_FILE] {
        if let Some(text) = bundle.get(name).and_then(Value::as_str) {
            std::fs::write(dir.join(name), text)
                .map_err(|e| format!("Failed to write {name}: {e}"))?;
        }
    }

    // Seed is hex-encoded - decode back to bytes.
    if let Some(hex_str) = bundle.get(SEED_FILE).and_then(Value::as_str) {
        let data = hex_decode(hex_str)
            .ok_or("Invalid hex for seed")?;
        std::fs::write(dir.join(SEED_FILE), data)
            .map_err(|e| format!("Failed to write seed: {e}"))?;
    }

    info!(label, ?src, "imported identity");
    Ok(label)
}

// ---- Proto <-> Wire conversion helpers ------------------------------

fn protocol_to_proto(protocol: PchatProtocol) -> i32 {
    match protocol {
        PchatProtocol::FancyV1PostJoin => mumble_tcp::PchatProtocol::FancyV1PostJoin as i32,
        PchatProtocol::FancyV1FullArchive => mumble_tcp::PchatProtocol::FancyV1FullArchive as i32,
        PchatProtocol::SignalV1 => mumble_tcp::PchatProtocol::SignalV1 as i32,
        PchatProtocol::ServerManaged => mumble_tcp::PchatProtocol::ServerManaged as i32,
        PchatProtocol::None => mumble_tcp::PchatProtocol::None as i32,
    }
}

fn proto_to_protocol(proto: Option<i32>) -> PchatProtocol {
    match proto.and_then(|v| mumble_tcp::PchatProtocol::try_from(v).ok()) {
        Some(mumble_tcp::PchatProtocol::FancyV1PostJoin) => PchatProtocol::FancyV1PostJoin,
        Some(mumble_tcp::PchatProtocol::FancyV1FullArchive) => PchatProtocol::FancyV1FullArchive,
        Some(mumble_tcp::PchatProtocol::SignalV1) => PchatProtocol::SignalV1,
        Some(mumble_tcp::PchatProtocol::ServerManaged) => PchatProtocol::ServerManaged,
        _ => PchatProtocol::FancyV1PostJoin,
    }
}

pub(crate) fn wire_key_announce_to_proto(w: &WireKeyAnnounce) -> mumble_tcp::PchatKeyAnnounce {
    mumble_tcp::PchatKeyAnnounce {
        algorithm_version: Some(w.algorithm_version as u32),
        identity_public: Some(w.identity_public.clone()),
        signing_public: Some(w.signing_public.clone()),
        cert_hash: Some(w.cert_hash.clone()),
        timestamp: Some(w.timestamp),
        signature: Some(w.signature.clone()),
        tls_signature: Some(w.tls_signature.clone()),
    }
}

fn proto_to_wire_key_announce(p: &mumble_tcp::PchatKeyAnnounce) -> WireKeyAnnounce {
    WireKeyAnnounce {
        algorithm_version: p.algorithm_version.unwrap_or(1) as u8,
        identity_public: p.identity_public.clone().unwrap_or_default(),
        signing_public: p.signing_public.clone().unwrap_or_default(),
        cert_hash: p.cert_hash.clone().unwrap_or_default(),
        timestamp: p.timestamp.unwrap_or(0),
        signature: p.signature.clone().unwrap_or_default(),
        tls_signature: p.tls_signature.clone().unwrap_or_default(),
    }
}

fn proto_to_wire_key_request(p: &mumble_tcp::PchatKeyRequest) -> WireKeyRequest {
    WireKeyRequest {
        channel_id: p.channel_id.unwrap_or(0),
        protocol: proto_protocol_to_wire_str(p.protocol),
        requester_hash: p.requester_hash.clone().unwrap_or_default(),
        requester_public: p.requester_public.clone().unwrap_or_default(),
        request_id: p.request_id.clone().unwrap_or_default(),
        timestamp: p.timestamp.unwrap_or(0),
        relay_cap: p.relay_cap.unwrap_or(3),
    }
}

/// Convert a wire key-exchange to the protobuf representation.
pub(crate) fn wire_key_exchange_to_proto_pub(w: &WireKeyExchange) -> mumble_tcp::PchatKeyExchange {
    mumble_tcp::PchatKeyExchange {
        channel_id: Some(w.channel_id),
        protocol: Some(wire_protocol_str_to_proto(&w.protocol)),
        epoch: Some(w.epoch),
        encrypted_key: Some(w.encrypted_key.clone()),
        sender_hash: Some(w.sender_hash.clone()),
        recipient_hash: Some(w.recipient_hash.clone()),
        request_id: w.request_id.clone(),
        timestamp: Some(w.timestamp),
        algorithm_version: Some(w.algorithm_version as u32),
        signature: Some(w.signature.clone()),
        parent_fingerprint: w.parent_fingerprint.clone(),
        epoch_fingerprint: if w.epoch_fingerprint.is_empty() { None } else { Some(w.epoch_fingerprint.clone()) },
        countersignature: w.countersignature.clone(),
        countersigner_hash: w.countersigner_hash.clone(),
    }
}

fn proto_to_wire_key_exchange(p: &mumble_tcp::PchatKeyExchange) -> WireKeyExchange {
    WireKeyExchange {
        channel_id: p.channel_id.unwrap_or(0),
        protocol: proto_protocol_to_wire_str(p.protocol),
        epoch: p.epoch.unwrap_or(0),
        encrypted_key: p.encrypted_key.clone().unwrap_or_default(),
        sender_hash: p.sender_hash.clone().unwrap_or_default(),
        recipient_hash: p.recipient_hash.clone().unwrap_or_default(),
        request_id: p.request_id.clone(),
        timestamp: p.timestamp.unwrap_or(0),
        algorithm_version: p.algorithm_version.unwrap_or(1) as u8,
        signature: p.signature.clone().unwrap_or_default(),
        parent_fingerprint: p.parent_fingerprint.clone(),
        epoch_fingerprint: p.epoch_fingerprint.clone().unwrap_or_default(),
        countersignature: p.countersignature.clone(),
        countersigner_hash: p.countersigner_hash.clone(),
    }
}

fn proto_protocol_to_wire_str(proto: Option<i32>) -> String {
    proto_to_protocol(proto).as_wire_str().to_string()
}

fn wire_protocol_str_to_proto(s: &str) -> i32 {
    let protocol = PchatProtocol::from_wire_str(s);
    protocol_to_proto(protocol)
}

// ---- Outbound: send key announce ------------------------------------

/// Send a key-announce to the server using native proto.
#[allow(dead_code, reason = "pchat feature is under development; will be called when key exchange is implemented")]
pub(crate) async fn send_key_announce(
    handle: &ClientHandle,
    key_manager: &KeyManager,
    cert_hash: &str,
) -> Result<(), String> {
    let now = now_millis();
    let wire_announce = key_manager.build_key_announce(cert_hash, now);
    let proto = wire_key_announce_to_proto(&wire_announce);

    handle
        .send(command::SendPchatKeyAnnounce { announce: proto })
        .await
        .map_err(|e| format!("send key-announce: {e}"))?;

    debug!(cert_hash, "sent pchat key-announce");
    Ok(())
}

// ---- Outbound: build encrypted plugin data (sync) ------------------

/// Encrypt a message and build a `PchatMessage` proto struct ready to send.
/// This is a synchronous operation (no network I/O) so it can be called
/// while holding the state lock.
#[allow(clippy::too_many_arguments, reason = "pchat message construction requires all security and routing fields")]
pub(crate) fn build_encrypted_pchat_message(
    pchat: &mut PchatState,
    channel_id: u32,
    protocol: PchatProtocol,
    message_id: &str,
    body: &str,
    sender_name: &str,
    sender_session: u32,
    timestamp: u64,
) -> Result<mumble_tcp::PchatMessage, String> {
    debug!(
        channel_id,
        ?protocol,
        message_id,
        timestamp,
        has_key = pchat.key_manager.has_key(channel_id, protocol),
        "pchat: build_encrypted_pchat_message"
    );
    let envelope = MessageEnvelope {
        body: body.to_string(),
        sender_name: sender_name.to_string(),
        sender_session,
        attachments: vec![],
    };

    let envelope_bytes = pchat
        .codec
        .encode(&envelope)
        .map_err(|e| format!("encode envelope: {e}"))?;

    let payload = pchat
        .key_manager
        .encrypt(protocol, channel_id, message_id, timestamp, &envelope_bytes)
        .map_err(|e| format!("encrypt message: {e}"))?;

    Ok(mumble_tcp::PchatMessage {
        message_id: Some(message_id.to_string()),
        channel_id: Some(channel_id),
        timestamp: Some(timestamp),
        sender_hash: Some(pchat.own_cert_hash.clone()),
        protocol: Some(protocol_to_proto(protocol)),
        envelope: Some(payload.ciphertext),
        epoch: payload.epoch,
        chain_index: payload.chain_index,
        epoch_fingerprint: Some(payload.epoch_fingerprint.to_vec()),
        replaces_id: None,
    })
}

// ---- Outbound: send encrypted message -------------------------------

/// Encrypt a message and send it as a native `PchatMessage` proto.
///
/// Per the spec (section 7.1), we send BOTH a plain `TextMessage` (for
/// backwards compat / real-time display) AND a `PchatMessage` proto
/// (for server storage). The `TextMessage` is sent by the caller;
/// this function handles only the encrypted proto path.
#[allow(dead_code, reason = "pchat feature is under development; will be called when encrypted send is wired up")]
#[allow(clippy::too_many_arguments, reason = "pchat message construction requires all security and routing fields")]
pub(crate) async fn send_encrypted_message(
    handle: &ClientHandle,
    pchat: &mut PchatState,
    channel_id: u32,
    protocol: PchatProtocol,
    message_id: &str,
    body: &str,
    sender_name: &str,
    sender_session: u32,
) -> Result<(), String> {
    let envelope = MessageEnvelope {
        body: body.to_string(),
        sender_name: sender_name.to_string(),
        sender_session,
        attachments: vec![],
    };

    let envelope_bytes = pchat
        .codec
        .encode(&envelope)
        .map_err(|e| format!("encode envelope: {e}"))?;

    let now = now_millis();

    let payload = pchat
        .key_manager
        .encrypt(protocol, channel_id, message_id, now, &envelope_bytes)
        .map_err(|e| format!("encrypt message: {e}"))?;

    let proto_msg = mumble_tcp::PchatMessage {
        message_id: Some(message_id.to_string()),
        channel_id: Some(channel_id),
        timestamp: Some(now),
        sender_hash: Some(pchat.own_cert_hash.clone()),
        protocol: Some(protocol_to_proto(protocol)),
        envelope: Some(payload.ciphertext),
        epoch: payload.epoch,
        chain_index: payload.chain_index,
        epoch_fingerprint: Some(payload.epoch_fingerprint.to_vec()),
        replaces_id: None,
    };

    handle
        .send(command::SendPchatMessage { message: proto_msg })
        .await
        .map_err(|e| format!("send pchat-msg: {e}"))?;

    debug!(message_id, channel_id, "sent encrypted pchat message");
    Ok(())
}

// ---- Outbound: send fetch request -----------------------------------

/// Send a `PchatFetch` proto to request stored messages.
#[allow(dead_code, reason = "pchat feature is under development; will be called when server-side fetch is implemented")]
pub(crate) async fn send_fetch(
    handle: &ClientHandle,
    channel_id: u32,
    before_id: Option<String>,
    limit: u32,
) -> Result<(), String> {
    let fetch = mumble_tcp::PchatFetch {
        channel_id: Some(channel_id),
        before_id,
        limit: Some(limit),
        after_id: None,
    };

    handle
        .send(command::SendPchatFetch { fetch })
        .await
        .map_err(|e| format!("send pchat-fetch: {e}"))?;

    debug!(channel_id, "sent pchat-fetch");
    Ok(())
}

// ---- Inbound: process incoming proto messages -----------------------

pub(crate) fn handle_proto_key_announce(shared: &Arc<Mutex<SharedState>>, msg: &mumble_tcp::PchatKeyAnnounce) {
    let wire = proto_to_wire_key_announce(msg);

    debug!(
        cert_hash = %wire.cert_hash,
        algo = wire.algorithm_version,
        "received pchat key-announce"
    );

    let Ok(mut state) = shared.lock() else { return };

    let mut should_push_keys = false;
    let peer_cert_hash = wire.cert_hash.clone();

    if let Some(ref mut pchat) = state.pchat {
        match pchat.key_manager.record_peer_key(&wire) {
            Ok(true) => {
                debug!(cert_hash = %wire.cert_hash, "recorded peer key");
                should_push_keys = true;
            }
            Ok(false) => debug!(cert_hash = %wire.cert_hash, "stale key-announce discarded"),
            Err(e) => warn!(cert_hash = %wire.cert_hash, "failed to record peer key: {e}"),
        }
    }

    // After successfully recording a peer's public key, instead of
    // proactively pushing our channel keys, emit a consent request to
    // the frontend so the user can decide whether to share.
    // We also collect channels that need a key-holder refresh so the
    // server can tell us whether this peer already holds the key (in
    // which case the consent prompt is auto-dismissed).
    let channels_to_query: Vec<u32>;

    if should_push_keys {
        let channels_for_peer = find_shareable_channels(&state, &peer_cert_hash);
        channels_to_query = channels_for_peer.clone();

        if !channels_for_peer.is_empty() {
            // Resolve peer name from current users.
            let peer_name = state
                .users
                .values()
                .find(|u| u.hash.as_deref() == Some(&peer_cert_hash))
                .map(|u| u.name.clone())
                .unwrap_or_else(|| peer_cert_hash.chars().take(8).collect());

            let app = state.tauri_app_handle.clone();

            for ch_id in channels_for_peer {
                // Avoid duplicate pending requests.
                let already_pending = state.pending_key_shares.iter().any(|p| {
                    p.channel_id == ch_id && p.peer_cert_hash == peer_cert_hash
                });
                if already_pending {
                    continue;
                }

                let pending = super::types::PendingKeyShare {
                    channel_id: ch_id,
                    peer_cert_hash: peer_cert_hash.clone(),
                    peer_name: peer_name.clone(),
                    request_id: None,
                };
                state.pending_key_shares.push(pending);

                if let Some(ref app) = app {
                    use tauri::Emitter;
                    let _ = app.emit(
                        "pchat-key-share-request",
                        super::types::KeyShareRequestPayload {
                            channel_id: ch_id,
                            peer_name: peer_name.clone(),
                            peer_cert_hash: peer_cert_hash.clone(),
                        },
                    );
                }

                debug!(
                    channel_id = ch_id,
                    peer = %peer_cert_hash,
                    "queued key-share consent request"
                );
            }
        }
    } else {
        channels_to_query = Vec::new();
    }

    // Drop the lock before sending network queries.
    drop(state);

    // Ask the server for fresh key-holder lists.  When the response arrives,
    // the handler will auto-dismiss consent prompts for peers that already
    // hold the key.
    for ch_id in channels_to_query {
        query_key_holders(shared, ch_id);
    }
}

/// Find `FullArchive` channel IDs where `peer_cert_hash` is present and we hold the key.
fn find_shareable_channels(
    state: &SharedState,
    peer_cert_hash: &str,
) -> Vec<u32> {
    let Some(ref pchat) = state.pchat else {
        return Vec::new();
    };

    let peer_channel_ids: Vec<u32> = state
        .users
        .values()
        .filter(|u| u.hash.as_deref() == Some(peer_cert_hash))
        .map(|u| u.channel_id)
        .collect();

    peer_channel_ids
        .into_iter()
        .filter(|&ch_id| {
            let is_full_archive = state
                .channels
                .get(&ch_id)
                .and_then(|ch| ch.pchat_protocol)
                == Some(PchatProtocol::FancyV1FullArchive);
            let has_key = pchat.key_manager.has_key(ch_id, PchatProtocol::FancyV1FullArchive);
            let already_holder = pchat.key_manager.key_holders(ch_id).contains(peer_cert_hash);
            is_full_archive && has_key && !already_holder
        })
        .collect()
}

/// Re-evaluate key sharing after a user moves into a channel.
///
/// Checks whether we hold the archive key for the given `FullArchive` channel
/// and whether any peers in that channel have known public keys.  For each
/// qualifying peer, a consent request is queued (if not already pending).
///
/// Call this:
/// - When a remote peer moves into a channel (after updating their state).
/// - When we move into a `FullArchive` channel (after deriving our key).
pub(crate) fn check_key_share_for_channel(shared: &Arc<Mutex<SharedState>>, channel_id: u32) {
    let Ok(mut state) = shared.lock() else { return };

    let is_full_archive = state
        .channels
        .get(&channel_id)
        .and_then(|c| c.pchat_protocol)
        == Some(PchatProtocol::FancyV1FullArchive);
    if !is_full_archive {
        return;
    }

    let Some(ref pchat) = state.pchat else { return };

    if !pchat.key_manager.has_key(channel_id, PchatProtocol::FancyV1FullArchive) {
        return;
    }

    let own_hash = pchat.own_cert_hash.clone();
    let holders = pchat.key_manager.key_holders(channel_id);

    // Collect peers in this channel for which we hold a peer key.
    let peers: Vec<(String, String)> = state
        .users
        .values()
        .filter(|u| u.channel_id == channel_id)
        .filter_map(|u| {
            let hash = u.hash.as_deref()?;
            if hash == own_hash {
                return None;
            }
            // Skip peers who are already known key holders.
            if holders.contains(hash) {
                return None;
            }
            // Only consider peers whose public key we already recorded.
            let _ = pchat.key_manager.get_peer(hash)?;
            Some((hash.to_owned(), u.name.clone()))
        })
        .collect();

    if peers.is_empty() {
        return;
    }

    let app = state.tauri_app_handle.clone();

    for (peer_cert_hash, peer_name) in peers {
        if state.pending_key_shares.iter().any(|p| {
            p.channel_id == channel_id && p.peer_cert_hash == peer_cert_hash
        }) {
            continue;
        }

        let pending = super::types::PendingKeyShare {
            channel_id,
            peer_cert_hash: peer_cert_hash.clone(),
            peer_name: peer_name.clone(),
            request_id: None,
        };
        state.pending_key_shares.push(pending);

        if let Some(ref app) = app {
            use tauri::Emitter;
            let _ = app.emit(
                "pchat-key-share-request",
                super::types::KeyShareRequestPayload {
                    channel_id,
                    peer_name: peer_name.clone(),
                    peer_cert_hash: peer_cert_hash.clone(),
                },
            );
        }

        debug!(
            channel_id,
            peer = %peer_cert_hash,
            "queued key-share consent on channel move"
        );
    }
}

pub(crate) fn handle_proto_key_request(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatKeyRequest,
) {
    let wire_request = proto_to_wire_key_request(msg);

    debug!(
        channel_id = wire_request.channel_id,
        requester = %wire_request.requester_hash,
        request_id = %wire_request.request_id,
        "received pchat key-request"
    );

    let Ok(mut state) = shared.lock() else { return };

    let Some(ref pchat) = state.pchat else { return };

    // Check we actually have a key for this channel before queuing a
    // consent banner.
    if !pchat.key_manager.has_key(
        wire_request.channel_id,
        PchatProtocol::FancyV1FullArchive,
    ) {
        debug!(channel_id = wire_request.channel_id, "no key to share for this channel");
        return;
    }

    let peer_cert_hash = wire_request.requester_hash.clone();
    let ch_id = wire_request.channel_id;
    let request_id = wire_request.request_id.clone();

    // Skip requests from users who are not currently online. When they
    // reconnect the server will re-trigger the request for active holders.
    let requester_online = state
        .users
        .values()
        .any(|u| u.hash.as_deref() == Some(peer_cert_hash.as_str()));
    if !requester_online {
        debug!(channel_id = ch_id, peer = %peer_cert_hash, "ignoring key-request from offline user");
        return;
    }

    // Skip requests from users who are already known key holders.
    if pchat.key_manager.key_holders(ch_id).contains(&peer_cert_hash) {
        debug!(channel_id = ch_id, peer = %peer_cert_hash, "ignoring key-request from existing holder");
        return;
    }

    // Avoid duplicate pending requests for the same channel + peer.
    let already_pending = state.pending_key_shares.iter().any(|p| {
        p.channel_id == ch_id && p.peer_cert_hash == peer_cert_hash
    });
    if already_pending {
        debug!(channel_id = ch_id, peer = %peer_cert_hash, "key-request consent already pending");
        return;
    }

    // Resolve peer name from current users.
    let peer_name = state
        .users
        .values()
        .find(|u| u.hash.as_deref() == Some(&peer_cert_hash))
        .map(|u| u.name.clone())
        .unwrap_or_else(|| peer_cert_hash.chars().take(8).collect());

    let pending = super::types::PendingKeyShare {
        channel_id: ch_id,
        peer_cert_hash: peer_cert_hash.clone(),
        peer_name: peer_name.clone(),
        request_id: Some(request_id),
    };
    state.pending_key_shares.push(pending);

    if let Some(ref app) = state.tauri_app_handle {
        use tauri::Emitter;
        let _ = app.emit(
            "pchat-key-share-request",
            super::types::KeyShareRequestPayload {
                channel_id: ch_id,
                peer_name: peer_name.clone(),
                peer_cert_hash: peer_cert_hash.clone(),
            },
        );
    }

    debug!(
        channel_id = ch_id,
        peer = %peer_cert_hash,
        "queued key-share consent request (key-request path)"
    );
}

#[allow(clippy::too_many_lines, reason = "key exchange handler covers unencrypted fetch, archive key derivation, peer challenge, and epoch resolution")]
pub(crate) fn handle_proto_key_exchange(shared: &Arc<Mutex<SharedState>>, msg: &mumble_tcp::PchatKeyExchange) {
    let wire_exchange = proto_to_wire_key_exchange(msg);

    debug!(
        channel_id = wire_exchange.channel_id,
        sender = %wire_exchange.sender_hash,
        epoch = wire_exchange.epoch,
        "received pchat key-exchange"
    );

    let channel_id = wire_exchange.channel_id;
    let protocol = PchatProtocol::from_wire_str(&wire_exchange.protocol);
    let request_id = wire_exchange.request_id.clone();

    let Ok(mut state) = shared.lock() else { return };

    let mut key_accepted = false;

    if let Some(ref mut pchat) = state.pchat {
        match pchat.key_manager.receive_key_exchange(&wire_exchange, None) {
            Ok(()) => {
                debug!(
                    channel_id = wire_exchange.channel_id,
                    epoch = wire_exchange.epoch,
                    "accepted key-exchange"
                );

                // The sender clearly holds the key -- record them as a holder
                // so we don't prompt consent for them again.
                pchat.key_manager.record_key_holder(
                    channel_id,
                    wire_exchange.sender_hash.clone(),
                );

                // For FullArchive, receive_key_exchange puts the key into
                // pending_consensus (when request_id is present). We must
                // evaluate consensus immediately to promote it into
                // archive_keys so that has_key() returns true.
                if protocol == PchatProtocol::FancyV1FullArchive {
                    if let Some(ref rid) = request_id {
                        match pchat.key_manager.evaluate_consensus(rid, channel_id, &[]) {
                            Ok((trust, Some(_key))) => {
                                debug!(
                                    channel_id,
                                    ?trust,
                                    "accepted archive key via consensus"
                                );
                                key_accepted = true;
                            }
                            Ok((_, None)) => {
                                warn!(channel_id, "consensus produced no key");
                            }
                            Err(e) => {
                                warn!(channel_id, "evaluate_consensus failed: {e}");
                            }
                        }
                    } else {
                        // No request_id means direct acceptance (already stored
                        // in archive_keys by receive_key_exchange).
                        key_accepted = pchat.key_manager.has_key(channel_id, protocol);
                    }
                } else {
                    key_accepted = pchat.key_manager.has_key(channel_id, protocol);
                }
            }
            Err(e) => {
                warn!(
                    channel_id = wire_exchange.channel_id,
                    "key-exchange rejected: {e}"
                );
            }
        }
    }

    // After accepting a key, re-decrypt any placeholder messages that
    // were stored before the key arrived (race between fetch-resp /
    // msg-deliver and key-exchange).
    if key_accepted {
        // We now hold the key -- record ourselves as a holder.
        if let Some(ref mut pchat) = state.pchat {
            pchat.key_manager.record_key_holder(
                channel_id,
                pchat.own_cert_hash.clone(),
            );
        }

        // The sender already has the key, so remove any pending consent
        // for sharing with them (they don't need it from us).
        let before_len = state.pending_key_shares.len();
        state.pending_key_shares.retain(|p| {
            !(p.channel_id == channel_id && p.peer_cert_hash == wire_exchange.sender_hash)
        });

        // Notify the frontend so it drops the stale "Share Key" banner.
        if state.pending_key_shares.len() != before_len {
            if let Some(ref app) = state.tauri_app_handle {
                use tauri::Emitter;
                let remaining: Vec<_> = state
                    .pending_key_shares
                    .iter()
                    .filter(|p| p.channel_id == channel_id)
                    .cloned()
                    .collect();
                let _ = app.emit(
                    "pchat-key-share-requests-changed",
                    super::types::KeyShareRequestsChangedPayload {
                        channel_id,
                        pending: remaining,
                    },
                );
            }
        }

        // Extract key data and identity_dir for disk persistence
        // before we drop the lock.
        let persist_info = if protocol == PchatProtocol::FancyV1FullArchive {
            state.pchat.as_ref().and_then(|p| {
                let (key, _trust) = p.key_manager.get_archive_key(channel_id)?;
                let originator = p.key_manager.get_channel_originator(channel_id)
                    .map(String::from);
                let dir = p.identity_dir.clone()?;
                Some((dir, key, originator))
            })
        } else {
            None
        };

        // Notify the frontend that the revoked key has been replaced.
        if let Some(ref app) = state.tauri_app_handle {
            use tauri::Emitter;
            let _ = app.emit(
                "pchat-key-restored",
                super::types::PchatKeyRevokedPayload { channel_id },
            );
        }

        retry_decrypt_pending_messages(&mut state, channel_id, protocol);

        // Drop the mutex before calling send_key_holder_report (which
        // re-acquires it briefly to read cert_hash + client_handle).
        drop(state);
        send_key_holder_report(shared, channel_id);

        // Persist the accepted archive key to disk (outside the lock).
        if let Some((dir, key, originator)) = persist_info {
            persist_archive_key(&dir, channel_id, &key, originator.as_deref());
        }
    }
}

/// Re-decrypt messages that were stored as "[Encrypted message - awaiting key]"
/// because the key had not yet arrived. Called after a key exchange is accepted.
///
/// Instead of re-fetching from the server, we rely on the fact that the
/// encrypted `PchatMsg` bodies are NOT stored locally. So we remove the
/// placeholder messages and clear the channel from `fetched_channels` so
/// the next channel visit (or an explicit re-fetch) pulls the history again
/// with the correct key.
fn retry_decrypt_pending_messages(
    state: &mut SharedState,
    channel_id: u32,
    _protocol: PchatProtocol,
) {
    let has_placeholders = state
        .messages
        .get(&channel_id)
        .is_some_and(|msgs| msgs.iter().any(|m| m.body == "[Encrypted message - awaiting key]"));

    if !has_placeholders {
        return;
    }

    debug!(
        channel_id,
        "removing placeholder messages and re-fetching after key exchange"
    );

    // Remove placeholder messages so re-fetch can replace them.
    if let Some(msgs) = state.messages.get_mut(&channel_id) {
        msgs.retain(|m| m.body != "[Encrypted message - awaiting key]");
    }

    // Allow re-fetching this channel's history.
    if let Some(ref mut pchat) = state.pchat {
        let _ = pchat.fetched_channels.remove(&channel_id);
    }

    // Spawn an async re-fetch so the messages are pulled now with the correct key.
    let handle = state.client_handle.clone();
    if let Some(handle) = handle {
        let _refetch_task = tokio::spawn(async move {
            let fetch = mumble_tcp::PchatFetch {
                channel_id: Some(channel_id),
                before_id: None,
                limit: Some(50),
                after_id: None,
            };
            if let Err(e) = handle
                .send(command::SendPchatFetch { fetch })
                .await
            {
                warn!(channel_id, "re-fetch after key exchange failed: {e}");
            } else {
                debug!(channel_id, "sent pchat re-fetch after key exchange");
            }
        });
    }
}

/// Cache a decrypted `SignalV1` message in the local encrypted store.
fn cache_signal_message(pchat: &mut PchatState, msg: CachedMessage) {
    if let Some(ref mut cache) = pchat.local_cache {
        cache.insert(msg);
    }
}

/// Attempt to decrypt and decode a real-time `PchatMessageDeliver` envelope.
///
/// Returns `Some((body, sender_name, decrypted))` on success or when a
/// placeholder is generated for a failed decryption.  Returns `None` when
/// the envelope is malformed and should be silently dropped.
fn decrypt_deliver_envelope(
    pchat: &mut PchatState,
    protocol: PchatProtocol,
    sender_hash: &str,
    channel_id: u32,
    message_id: &str,
    timestamp: u64,
    envelope_bytes: Vec<u8>,
) -> Option<(String, String, bool)> {
    let decrypt_result = if protocol == PchatProtocol::SignalV1 {
        pchat
            .key_manager
            .decrypt_signal(sender_hash, channel_id, &envelope_bytes)
    } else {
        let payload = mumble_protocol::persistent::keys::EncryptedPayload {
            ciphertext: envelope_bytes.clone(),
            epoch: None,
            chain_index: None,
            epoch_fingerprint: [0u8; 8],
        };
        pchat
            .key_manager
            .decrypt(protocol, channel_id, message_id, timestamp, &payload)
    };

    match decrypt_result {
        Ok(plaintext) => {
            debug!(message_id = %message_id, plaintext_len = plaintext.len(), "pchat msg-deliver: decrypted OK");
            match pchat.codec.decode::<MessageEnvelope>(&plaintext) {
                Ok(env) => Some((env.body, env.sender_name, true)),
                Err(e) => {
                    warn!(message_id = %message_id, "failed to decode envelope: {e}");
                    None
                }
            }
        }
        Err(e) => {
            warn!(
                message_id = %message_id,
                channel_id,
                sender = %sender_hash,
                ciphertext_len = envelope_bytes.len(),
                has_key = pchat.key_manager.has_key(channel_id, protocol),
                "failed to decrypt message: {e}"
            );
            if protocol == PchatProtocol::SignalV1 {
                // Cap stash size to avoid unbounded growth.
                const MAX_STASHED: usize = 50;
                if pchat.pending_signal_envelopes.len() < MAX_STASHED {
                    pchat.pending_signal_envelopes.push(PendingSignalEnvelope {
                        message_id: message_id.to_owned(),
                        channel_id,
                        timestamp,
                        sender_hash: sender_hash.to_owned(),
                        envelope_bytes,
                    });
                    debug!(
                        message_id = %message_id,
                        channel_id,
                        sender = %sender_hash,
                        "stashed signal envelope for later retry"
                    );
                } else {
                    warn!("signal stash full ({MAX_STASHED}), dropping envelope");
                }
            }
            Some((
                "[Encrypted message - awaiting key]".to_string(),
                sender_hash.to_owned(),
                false,
            ))
        }
    }
}

pub(crate) fn handle_proto_msg_deliver(shared: &Arc<Mutex<SharedState>>, msg: &mumble_tcp::PchatMessageDeliver) {
    let message_id = msg.message_id.clone().unwrap_or_default();
    let channel_id = msg.channel_id.unwrap_or(0);
    let timestamp = msg.timestamp.unwrap_or(0);
    let sender_hash = msg.sender_hash.clone().unwrap_or_default();
    let protocol = proto_to_protocol(msg.protocol);
    let envelope_bytes = msg.envelope.clone().unwrap_or_default();
    let replaces_id = msg.replaces_id.clone();

    debug!(data_len = envelope_bytes.len(), "pchat: handle_proto_msg_deliver entry");

    debug!(
        message_id = %message_id,
        channel_id,
        sender = %sender_hash,
        "received pchat msg-deliver"
    );

    let Ok(mut state) = shared.lock() else { return };

    // The server never echoes PchatMessageDeliver back to the sender
    // (broadcastPchatMessageDeliver uses excludeSession). Our own
    // messages are stored locally by send_message() with is_own=true,
    // so any deliver we receive is guaranteed to be from someone else.
    let is_own = false;

    let Some(pchat) = state.pchat.as_mut() else { return };

    let Some((body, sender_name, decrypted)) = decrypt_deliver_envelope(
        pchat, protocol, &sender_hash, channel_id, &message_id, timestamp, envelope_bytes,
    ) else {
        return;
    };

    // Cache successfully decrypted SignalV1 messages locally.
    if protocol == PchatProtocol::SignalV1 && decrypted {
        cache_signal_message(pchat, CachedMessage {
            message_id: message_id.clone(), channel_id, timestamp,
            sender_hash: sender_hash.clone(), sender_name: sender_name.clone(),
            body: body.clone(), is_own: false,
        });
    }

    let sender_session = state
        .users
        .values()
        .find(|u| u.hash.as_deref() == Some(&sender_hash))
        .map(|u| u.session);

    let chat_msg = ChatMessage {
        sender_session,
        sender_name,
        body,
        channel_id,
        is_own,
        dm_session: None,
        group_id: None,
        message_id: Some(message_id.clone()),
        timestamp: Some(timestamp),
        is_legacy: false,
    };

    if let Some(ref replaces_id) = replaces_id {
        if let Some(msgs) = state.messages.get_mut(&channel_id) {
            if let Some(pos) = msgs
                .iter()
                .position(|m| m.message_id.as_deref() == Some(replaces_id))
            {
                msgs[pos] = chat_msg;
                return;
            }
        }
    }

    if let Some(msgs) = state.messages.get(&channel_id) {
        if msgs
            .iter()
            .any(|m| m.message_id.as_deref() == Some(&message_id))
        {
            return;
        }
    }

    state
        .messages
        .entry(channel_id)
        .or_default()
        .push(chat_msg);
}

#[allow(clippy::too_many_lines, reason = "fetch response handler decrypts, decodes, deduplicates, and stores messages then updates UI")]
pub(crate) fn handle_proto_fetch_resp(shared: &Arc<Mutex<SharedState>>, msg: &mumble_tcp::PchatFetchResponse) {
    let channel_id = msg.channel_id.unwrap_or(0);
    let has_more = msg.has_more.unwrap_or(false);
    let total_stored = msg.total_stored.unwrap_or(0);

    debug!(data_len = msg.messages.len(), "pchat: handle_proto_fetch_resp entry");

    debug!(
        channel_id,
        count = msg.messages.len(),
        has_more,
        total = total_stored,
        "received pchat fetch-resp"
    );

    let Ok(mut state) = shared.lock() else { return };

    let Some(own_cert_hash) = state.pchat.as_ref().map(|p| p.own_cert_hash.clone()) else { return };

    // Decrypt each message and insert at the beginning (they're older)
    let mut decrypted_msgs: Vec<ChatMessage> = Vec::with_capacity(msg.messages.len());

    for proto_msg in &msg.messages {
        let Some(pchat) = state.pchat.as_mut() else { return };

        let msg_id = proto_msg.message_id.clone().unwrap_or_default();
        let msg_channel_id = proto_msg.channel_id.unwrap_or(0);
        let msg_timestamp = proto_msg.timestamp.unwrap_or(0);
        let msg_sender_hash = proto_msg.sender_hash.clone().unwrap_or_default();
        let protocol = proto_to_protocol(proto_msg.protocol);
        let has_key = pchat.key_manager.has_key(msg_channel_id, protocol);

        debug!(
            message_id = %msg_id,
            channel_id = msg_channel_id,
            timestamp = msg_timestamp,
            sender = %msg_sender_hash,
            envelope_len = proto_msg.envelope.as_ref().map(Vec::len).unwrap_or(0),
            has_key,
            "pchat fetch-resp: processing message"
        );

        let decrypt_result = if protocol == PchatProtocol::SignalV1 {
            pchat
                .key_manager
                .decrypt_signal(&msg_sender_hash, msg_channel_id, &proto_msg.envelope.clone().unwrap_or_default())
        } else {
            let payload = mumble_protocol::persistent::keys::EncryptedPayload {
                ciphertext: proto_msg.envelope.clone().unwrap_or_default(),
                epoch: proto_msg.epoch,
                chain_index: proto_msg.chain_index,
                epoch_fingerprint: proto_msg
                    .epoch_fingerprint
                    .clone()
                    .unwrap_or_default()
                    .try_into()
                    .unwrap_or([0u8; 8]),
            };
            pchat
                .key_manager
                .decrypt(protocol, msg_channel_id, &msg_id, msg_timestamp, &payload)
        };

        let (body, sender_name, decrypted) = match decrypt_result
        {
            Ok(plaintext) => {
                debug!(message_id = %msg_id, plaintext_len = plaintext.len(), "pchat fetch-resp: decrypted OK");
                match pchat.codec.decode::<MessageEnvelope>(&plaintext) {
                Ok(env) => (env.body, env.sender_name, true),
                Err(e) => {
                    warn!(message_id = %msg_id, "fetch-resp: decode envelope: {e}");
                    continue;
                }
            }
            },
            Err(e) => {
                warn!(message_id = %msg_id, channel_id = msg_channel_id, has_key, "fetch-resp: decrypt failed: {e}");
                (
                    "[Encrypted message - awaiting key]".to_string(),
                    msg_sender_hash.clone(),
                    false,
                )
            }
        };

        // Compare cert hashes to determine ownership.  Guard against
        // empty hashes: if either side is empty the comparison is
        // meaningless, so default to "not own".
        let is_own = !msg_sender_hash.is_empty()
            && !own_cert_hash.is_empty()
            && msg_sender_hash == own_cert_hash;

        // Cache successfully decrypted SignalV1 messages locally.
        if protocol == PchatProtocol::SignalV1 && decrypted {
            cache_signal_message(pchat, CachedMessage {
                message_id: msg_id.clone(), channel_id: msg_channel_id,
                timestamp: msg_timestamp, sender_hash: msg_sender_hash.clone(),
                sender_name: sender_name.clone(), body: body.clone(), is_own,
            });
        }

        debug!(
            message_id = %msg_id,
            msg_sender_hash = %msg_sender_hash,
            own_cert_hash = %own_cert_hash,
            is_own,
            sender_name = %sender_name,
            "pchat fetch-resp: is_own check"
        );

        let sender_session = state
            .users
            .values()
            .find(|u| u.hash.as_deref() == Some(&msg_sender_hash))
            .map(|u| u.session);

        decrypted_msgs.push(ChatMessage {
            sender_session,
            sender_name,
            body,
            channel_id: msg_channel_id,
            is_own,
            dm_session: None,
            group_id: None,
            message_id: Some(msg_id.clone()),
            timestamp: Some(msg_timestamp),
            is_legacy: false,
        });
    }

    if !decrypted_msgs.is_empty() {
        debug!(
            channel_id,
            new_count = decrypted_msgs.len(),
            "pchat fetch-resp: inserting decrypted messages"
        );
        let existing = state.messages.entry(channel_id).or_default();

        // De-duplicate: only add messages we don't already have.
        // Only compare messages that have an actual message_id — a None
        // id means "unknown" and should never match another None.
        let existing_ids: std::collections::HashSet<&str> = existing
            .iter()
            .filter_map(|m| m.message_id.as_deref())
            .collect();

        let mut new_msgs: Vec<ChatMessage> = decrypted_msgs
            .into_iter()
            .filter(|m| match m.message_id.as_deref() {
                Some(id) => !existing_ids.contains(id),
                None => true, // always keep messages without an id
            })
            .collect();

        // Prepend historical messages (they're older) then append existing
        new_msgs.append(existing);
        *existing = new_msgs;

        // Sort by timestamp to maintain chronological order
        existing.sort_by_key(|m| m.timestamp.unwrap_or(0));

        debug!(
            channel_id,
            total_messages = existing.len(),
            "pchat fetch-resp: messages after merge+sort"
        );
    } else {
        debug!(channel_id, "pchat fetch-resp: no messages to insert (all filtered/empty)");
    }
}

pub(crate) fn handle_proto_ack(msg: &mumble_tcp::PchatAck) {
    let message_ids = &msg.message_ids;
    let status = msg.status.unwrap_or(0);
    let reason = msg.reason.as_deref();

    if status == mumble_tcp::PchatAckStatus::PchatAckRejected as i32
        || status == mumble_tcp::PchatAckStatus::PchatAckQuotaExceeded as i32
    {
        warn!(
            ?message_ids,
            status,
            reason = ?reason,
            "pchat message rejected by server"
        );
    } else {
        debug!(
            ?message_ids,
            status,
            "received pchat ack"
        );
    }
}

/// Handle a `PchatDeleteMessages` broadcast from the server.
///
/// Evicts matching messages from the local in-memory store based on the
/// deletion criteria (message IDs, time range, sender hash).
pub(crate) fn handle_proto_delete_messages(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatDeleteMessages,
) {
    let channel_id = msg.channel_id.unwrap_or(0);
    let Ok(mut state) = shared.lock() else {
        return;
    };

    let Some(messages) = state.messages.get_mut(&channel_id) else {
        debug!(channel_id, "pchat delete: no local messages for channel");
        return;
    };

    let before = messages.len();

    let ids = &msg.message_ids;
    let time_range = msg.time_range.as_ref();
    let sender_hash = msg.sender_hash.as_deref();

    messages.retain(|m| {
        // By specific message IDs
        if !ids.is_empty() {
            if let Some(ref mid) = m.message_id {
                if ids.iter().any(|id| id == mid) {
                    return false;
                }
            }
        }
        // By time range
        if let Some(range) = time_range {
            if let Some(ts) = m.timestamp {
                let after_from = range.from.is_none_or(|f| ts >= f);
                let before_to = range.to.is_none_or(|t| ts <= t);
                if after_from && before_to {
                    return false;
                }
            }
        }
        // By sender hash - match against sender_name as a fallback,
        // since ChatMessage does not store the cert hash directly.
        if let Some(hash) = sender_hash {
            if m.sender_name == hash {
                return false;
            }
        }
        true
    });

    let removed = before - messages.len();
    debug!(channel_id, removed, "pchat delete: evicted messages from local store");
}

// ---- Offline queue drain handler ------------------------------------

/// Handle a `PchatOfflineQueueDrain` from the server.
///
/// The server sends a batch of messages that were queued while we were
/// offline.  Each entry is a full `PchatMessageDeliver`.  We decrypt,
/// deduplicate, insert into the local message store, and then send a
/// `PchatAck` back so the server can remove those entries from the queue.
pub(crate) fn handle_proto_offline_queue_drain(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatOfflineQueueDrain,
) {
    let channel_id = msg.channel_id.unwrap_or(0);

    debug!(
        channel_id,
        count = msg.messages.len(),
        "received offline queue drain"
    );

    if msg.messages.is_empty() {
        return;
    }

    let Ok(mut state) = shared.lock() else { return };
    let Some(pchat) = state.pchat.as_mut() else { return };

    // Phase 1: decrypt all messages while holding the pchat borrow.
    let decrypted_msgs = decrypt_offline_batch(pchat, channel_id, &msg.messages);

    // Phase 2: insert messages into state (pchat borrow is no longer needed).
    let acked_ids = insert_offline_messages(&mut state, channel_id, &decrypted_msgs);

    // Sort the channel's messages by timestamp so offline messages
    // appear in chronological order.
    if let Some(msgs) = state.messages.get_mut(&channel_id) {
        msgs.sort_by_key(|m| m.timestamp.unwrap_or(0));
    }

    // Send PchatAck to confirm receipt so server can delete from queue.
    if !acked_ids.is_empty() {
        send_offline_queue_ack(&state, channel_id, acked_ids);
    }
}

/// Intermediate result after decrypting a single offline-queued message.
struct DecryptedOfflineMsg {
    message_id: String,
    timestamp: u64,
    sender_hash: String,
    body: String,
    sender_name: String,
}

/// Decrypt and cache a batch of `PchatMessageDeliver` entries from an
/// offline queue drain.  Returns the successfully processed messages.
fn decrypt_offline_batch(
    pchat: &mut PchatState,
    channel_id: u32,
    messages: &[mumble_tcp::PchatMessageDeliver],
) -> Vec<DecryptedOfflineMsg> {
    let mut results = Vec::with_capacity(messages.len());

    for deliver in messages {
        let message_id = deliver.message_id.clone().unwrap_or_default();
        let timestamp = deliver.timestamp.unwrap_or(0);
        let sender_hash = deliver.sender_hash.clone().unwrap_or_default();
        let protocol = proto_to_protocol(deliver.protocol);
        let envelope_bytes = deliver.envelope.clone().unwrap_or_default();

        let decrypt_result = if protocol == PchatProtocol::SignalV1 {
            pchat
                .key_manager
                .decrypt_signal(&sender_hash, channel_id, &envelope_bytes)
        } else {
            let payload = mumble_protocol::persistent::keys::EncryptedPayload {
                ciphertext: envelope_bytes.clone(),
                epoch: None,
                chain_index: None,
                epoch_fingerprint: [0u8; 8],
            };
            pchat
                .key_manager
                .decrypt(protocol, channel_id, &message_id, timestamp, &payload)
        };

        let (body, sender_name, decrypted) = match decrypt_result {
            Ok(plaintext) => {
                debug!(message_id = %message_id, "offline drain: decrypted OK");
                match pchat.codec.decode::<MessageEnvelope>(&plaintext) {
                    Ok(env) => (env.body, env.sender_name, true),
                    Err(e) => {
                        warn!(message_id = %message_id, "offline drain: failed to decode envelope: {e}");
                        continue;
                    }
                }
            }
            Err(e) => {
                warn!(message_id = %message_id, channel_id, "offline drain: failed to decrypt: {e}");
                // For SignalV1, stash the encrypted envelope so we can retry
                // once the sender's distribution key arrives.
                if protocol == PchatProtocol::SignalV1 {
                    pchat.pending_signal_envelopes.push(PendingSignalEnvelope {
                        message_id: message_id.clone(),
                        channel_id,
                        timestamp,
                        sender_hash: sender_hash.clone(),
                        envelope_bytes,
                    });
                    debug!(
                        message_id = %message_id,
                        channel_id,
                        sender = %sender_hash,
                        "stashed signal envelope (offline drain) for later retry"
                    );
                }
                (
                    "[Encrypted message - awaiting key]".to_string(),
                    sender_hash.clone(),
                    false,
                )
            }
        };

        if protocol == PchatProtocol::SignalV1 && decrypted {
            cache_signal_message(pchat, CachedMessage {
                message_id: message_id.clone(),
                channel_id,
                timestamp,
                sender_hash: sender_hash.clone(),
                sender_name: sender_name.clone(),
                body: body.clone(),
                is_own: false,
            });
        }

        results.push(DecryptedOfflineMsg {
            message_id,
            timestamp,
            sender_hash,
            body,
            sender_name,
        });
    }

    results
}

/// Insert decrypted offline messages into the state, deduplicating against
/// existing messages.  Returns the IDs of all processed messages (for ack).
fn insert_offline_messages(
    state: &mut SharedState,
    channel_id: u32,
    decrypted: &[DecryptedOfflineMsg],
) -> Vec<String> {
    let mut acked_ids: Vec<String> = Vec::with_capacity(decrypted.len());

    for dm in decrypted {
        if let Some(msgs) = state.messages.get(&channel_id) {
            if msgs
                .iter()
                .any(|m| m.message_id.as_deref() == Some(&dm.message_id))
            {
                acked_ids.push(dm.message_id.clone());
                continue;
            }
        }

        let sender_session = state
            .users
            .values()
            .find(|u| u.hash.as_deref() == Some(&dm.sender_hash))
            .map(|u| u.session);

        let chat_msg = ChatMessage {
            sender_session,
            sender_name: dm.sender_name.clone(),
            body: dm.body.clone(),
            channel_id,
            is_own: false,
            dm_session: None,
            group_id: None,
            message_id: Some(dm.message_id.clone()),
            timestamp: Some(dm.timestamp),
            is_legacy: false,
        };

        state
            .messages
            .entry(channel_id)
            .or_default()
            .push(chat_msg);

        acked_ids.push(dm.message_id.clone());
    }

    acked_ids
}

/// Send a `PchatAck` for offline-queued messages so the server can remove
/// them from the queue.
fn send_offline_queue_ack(state: &SharedState, channel_id: u32, acked_ids: Vec<String>) {
    let Some(handle) = state.client_handle.clone() else {
        return;
    };
    let ack = mumble_tcp::PchatAck {
        message_ids: acked_ids.clone(),
        status: Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32),
        reason: None,
        channel_id: Some(channel_id),
    };
    let _ack_task = tokio::spawn(async move {
        if let Err(e) = handle.send(command::SendPchatAck { ack }).await {
            warn!(channel_id, "failed to send offline queue ack: {e}");
        } else {
            debug!(
                channel_id,
                count = acked_ids.len(),
                "sent offline queue ack"
            );
        }
    });
}

// ---- Key-possession challenge handlers ------------------------------

/// Handle a `PchatKeyChallenge` from the server.
///
/// The server asks us to prove we hold the real archive key for `channel_id`
/// by computing `HMAC-SHA256(archive_key, challenge)` and sending the proof
/// back as a `PchatKeyChallengeResponse`.
pub(crate) fn handle_proto_key_challenge(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatKeyChallenge,
) {
    let channel_id = msg.channel_id.unwrap_or(0);
    let challenge = msg.challenge.as_deref().unwrap_or_default();

    if challenge.is_empty() {
        warn!(channel_id, "received empty challenge from server, ignoring");
        return;
    }

    let (handle, proof) = {
        let s = shared.lock().ok();
        let h = s.as_ref().and_then(|s| s.client_handle.clone());
        let proof = s
            .as_ref()
            .and_then(|s| s.pchat.as_ref())
            .and_then(|p| p.key_manager.compute_challenge_proof(channel_id, challenge));
        (h, proof)
    };

    match (handle, proof) {
        (Some(handle), Some(proof)) => {
            debug!(channel_id, "responding to key-possession challenge");
            let _challenge_response_task = tokio::spawn(async move {
                let response = mumble_tcp::PchatKeyChallengeResponse {
                    channel_id: Some(channel_id),
                    proof: Some(proof.to_vec()),
                };
                if let Err(e) = handle
                    .send(command::SendPchatKeyChallengeResponse { response })
                    .await
                {
                    warn!(channel_id, "failed to send challenge response: {e}");
                }
            });
        }
        (_, None) => {
            warn!(
                channel_id,
                "no archive key for channel, cannot respond to challenge"
            );
        }
        (None, _) => {
            warn!("no client handle, cannot respond to challenge");
        }
    }
}

/// Handle a `PchatKeyChallengeResult` from the server.
///
/// If `passed == true`, our key is verified and we are accepted as a holder.
/// If `passed == false`, we hold a wrong key: remove it from memory and disk
/// so we don't keep decrypting with invalid keying material.
pub(crate) fn handle_proto_key_challenge_result(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatKeyChallengeResult,
) {
    let channel_id = msg.channel_id.unwrap_or(0);
    let passed = msg.passed.unwrap_or(false);

    if passed {
        debug!(channel_id, "key-possession challenge passed");
        return;
    }

    warn!(
        channel_id,
        "key-possession challenge FAILED - discarding archive key"
    );

    let (identity_dir, app) = {
        let mut s = shared.lock().ok();
        let dir = s
            .as_ref()
            .and_then(|s| s.pchat.as_ref())
            .and_then(|p| p.identity_dir.clone());
        let app_handle = s.as_ref().and_then(|s| s.tauri_app_handle.clone());
        // Remove all keying material for the channel from memory.
        if let Some(ref mut s) = s {
            if let Some(ref mut pchat) = s.pchat {
                pchat.key_manager.remove_channel(channel_id);
            }
            // Clear pending key-share requests for this channel
            // (we can no longer share a key we don't have).
            let before_len = s.pending_key_shares.len();
            s.pending_key_shares.retain(|p| p.channel_id != channel_id);
            if s.pending_key_shares.len() != before_len {
                if let Some(ref app) = app_handle {
                    use tauri::Emitter;
                    let _ = app.emit(
                        "pchat-key-share-requests-changed",
                        super::types::KeyShareRequestsChangedPayload {
                            channel_id,
                            pending: vec![],
                        },
                    );
                }
            }
        }
        (dir, app_handle)
    };

    // Remove the persisted archive key from disk.
    if let Some(dir) = identity_dir {
        delete_persisted_archive_key(&dir, channel_id);
    }

    // Notify the frontend so it can disable input and hide stale UI.
    if let Some(app) = app {
        use tauri::Emitter;
        let _ = app.emit(
            "pchat-key-revoked",
            super::types::PchatKeyRevokedPayload { channel_id },
        );
    }
}

// ---- Helper ---------------------------------------------------------

/// Report to the server that we hold the E2EE key for a channel.
///
/// Extracts own cert hash and client handle from the shared state,
/// records ourselves as a key holder locally, and returns the prepared
/// report and handle. Returns `None` if state is unavailable.
fn prepare_key_holder_report(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
) -> Option<(ClientHandle, mumble_tcp::PchatKeyHolderReport)> {
    let (handle, hash) = {
        let mut s = shared.lock().ok();
        let h = s.as_ref().and_then(|s| s.client_handle.clone());
        let hash = s.as_ref().and_then(|s| s.pchat.as_ref().map(|p| p.own_cert_hash.clone()));

        // Verify we actually hold a usable key before reporting.
        // For SignalV1 this checks that the signal bridge is loaded;
        // for other modes it checks archive/epoch key presence.
        let mode = s
            .as_ref()
            .and_then(|s| s.channels.get(&channel_id).and_then(|c| c.pchat_protocol));
        if let (Some(ref s), Some(mode)) = (&s, mode) {
            if let Some(ref pchat) = s.pchat {
                if !pchat.key_manager.has_key(channel_id, mode) {
                    warn!(channel_id, ?mode, "not reporting as key holder: no usable key");
                    return None;
                }
            }
        }

        // Record ourselves as holder locally so consent checks skip us.
        if let (Some(ref mut s), Some(ref hash)) = (&mut s, &hash) {
            if let Some(ref mut pchat) = s.pchat {
                pchat.key_manager.record_key_holder(channel_id, hash.clone());
            }
        }
        (h, hash)
    };
    match (handle, hash) {
        (Some(handle), Some(hash)) => {
            let report = mumble_tcp::PchatKeyHolderReport {
                channel_id: Some(channel_id),
                cert_hash: Some(hash),
                takeover_mode: None,
            };
            Some((handle, report))
        }
        _ => None,
    }
}

/// Report to the server that we hold the E2EE key for a channel.
///
/// Async variant: the caller `.await`s the network send so the report
/// reaches the command queue before any subsequent commands (e.g. fetch).
/// Use this in async contexts (`server_sync`, `user_state` handlers).
pub(crate) async fn send_key_holder_report_async(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
) {
    if let Some((handle, report)) = prepare_key_holder_report(shared, channel_id) {
        if let Err(e) = handle
            .send(command::SendPchatKeyHolderReport { report })
            .await
        {
            warn!(channel_id, "failed to report key holder: {e}");
        } else {
            debug!(channel_id, "reported self as key holder");
        }
    }
}

/// Report to the server that we hold the E2EE key for a channel.
///
/// Fire-and-forget variant: spawns a task for the send.
/// Use this in synchronous contexts where `.await` is not possible.
pub(crate) fn send_key_holder_report(shared: &Arc<Mutex<SharedState>>, channel_id: u32) {
    if let Some((handle, report)) = prepare_key_holder_report(shared, channel_id) {
        let _key_holder_report_task = tokio::spawn(async move {
            if let Err(e) = handle
                .send(command::SendPchatKeyHolderReport { report })
                .await
            {
                warn!(channel_id, "failed to report key holder: {e}");
            } else {
                debug!(channel_id, "reported self as key holder");
            }
        });
    }
}

/// Request a key-ownership takeover for a channel (requires `KeyOwner` permission).
///
/// Sends a `PchatKeyHolderReport` with the given `takeover_mode`. The server will:
/// - `FullWipe`: Delete all stored messages, remove all known key holders,
///   reset the challenge state, and record the sender as the sole new key holder.
/// - `KeyOnly`: Remove all known key holders and reset the challenge state
///   without deleting messages, then record the sender as the sole new key holder.
///
/// On success the server responds with the updated `PchatKeyHoldersList`.
/// On failure the server sends `PermissionDenied`.
pub(crate) fn send_key_takeover(shared: &Arc<Mutex<SharedState>>, channel_id: u32, mode: mumble_tcp::pchat_key_holder_report::KeyTakeoverMode) {
    let (handle, hash) = {
        let s = shared.lock().ok();
        let h = s.as_ref().and_then(|s| s.client_handle.clone());
        let hash = s
            .as_ref()
            .and_then(|s| s.pchat.as_ref().map(|p| p.own_cert_hash.clone()));
        (h, hash)
    };
    let Some(handle) = handle else { return };
    let Some(hash) = hash else { return };

    let report = mumble_tcp::PchatKeyHolderReport {
        channel_id: Some(channel_id),
        cert_hash: Some(hash),
        takeover_mode: Some(mode as i32),
    };

    let _task = tokio::spawn(async move {
        if let Err(e) = handle
            .send(command::SendPchatKeyHolderReport { report })
            .await
        {
            warn!(channel_id, "failed to send key takeover: {e}");
        } else {
            debug!(channel_id, "sent key takeover");
        }
    });
}

/// Ask the server for the latest key holders of a channel.
///
/// Fire-and-forget: spawns a task for the network send.
/// When the response arrives the handler in `handler/pchat.rs` updates
/// the local cache and auto-dismisses stale "Share Key" consent prompts.
pub(crate) fn query_key_holders(shared: &Arc<Mutex<SharedState>>, channel_id: u32) {
    let handle = {
        let Ok(state) = shared.lock() else { return };
        state.client_handle.clone()
    };
    let Some(handle) = handle else { return };
    let query = mumble_tcp::PchatKeyHoldersQuery {
        channel_id: Some(channel_id),
    };
    let _query_task = tokio::spawn(async move {
        if let Err(e) = handle.send(command::SendPchatKeyHoldersQuery { query }).await {
            warn!(channel_id, "failed to query key holders: {e}");
        }
    });
}

pub(crate) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ---- Signal bridge loading ------------------------------------------

/// Attempt to load the Signal Protocol bridge DLL.
///
/// Searches for the platform-specific library name in several locations:
/// 1. Next to the executable (Windows installers, `AppImage`, dev mode)
/// 2. `../lib/fancy-mumble/` relative to the exe (Linux deb packages where
///    the binary is in `/usr/bin/` and resources in `/usr/lib/fancy-mumble/`)
/// 3. Extra search directory (e.g. Android `nativeLibraryDir`)
/// 4. On Android, bare filename as fallback (`dlopen` resolves it from the
///    app's native library directory automatically)
///
/// Returns `None` (with a warning) if the library is not found anywhere.
pub(crate) fn load_signal_bridge(
    own_cert_hash: &str,
    extra_search_dir: Option<&Path>,
) -> Option<Arc<SignalBridge>> {
    let lib_name = if cfg!(windows) {
        "signal_bridge.dll"
    } else if cfg!(target_os = "macos") {
        "libsignal_bridge.dylib"
    } else {
        "libsignal_bridge.so"
    };

    let mut candidates: Vec<PathBuf> = Vec::new();

    // Desktop: look next to the executable and in resource subdirectories
    #[cfg(not(target_os = "android"))]
    {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));

        if let Some(ref dir) = exe_dir {
            // 1. Next to the executable (dev mode, Windows NSIS/MSI)
            candidates.push(dir.join(lib_name));
            // 2. Tauri bundled resource subdirectory
            candidates.push(dir.join("signal-bridge").join(lib_name));
            // 3. Tauri resource dir for Linux deb: ../lib/fancy-mumble/
            candidates.push(dir.join("../lib/fancy-mumble").join(lib_name));
            candidates.push(
                dir.join("../lib/fancy-mumble/signal-bridge")
                    .join(lib_name),
            );
        }
    }

    // 3. Extra search directory
    if let Some(dir) = extra_search_dir {
        candidates.push(dir.join(lib_name));
    }

    // 4. Bare name as last resort (OS / Android linker search path)
    candidates.push(PathBuf::from(lib_name));

    // On Android, dlopen resolves bare filenames from the app's native
    // library directory, so we cannot rely on Path::exists(). Try each
    // candidate with SignalBridge::new directly.
    #[cfg(target_os = "android")]
    {
        for candidate in &candidates {
            match SignalBridge::new(candidate, own_cert_hash) {
                Ok(bridge) => {
                    info!(?candidate, "loaded signal bridge");
                    return Some(Arc::new(bridge));
                }
                Err(e) => {
                    debug!(?candidate, "signal bridge candidate failed: {e}");
                }
            }
        }
        warn!(
            ?candidates,
            "signal bridge library not found; SignalV1 channels will not work"
        );
        return None;
    }

    #[cfg(not(target_os = "android"))]
    {
        let lib_path = candidates.iter().find(|p| p.exists());

        let Some(lib_path) = lib_path else {
            warn!(
                ?candidates,
                "signal bridge library not found; SignalV1 channels will not work"
            );
            return None;
        };

        match SignalBridge::new(lib_path, own_cert_hash) {
            Ok(bridge) => {
                info!(?lib_path, "loaded signal bridge");
                Some(Arc::new(bridge))
            }
            Err(e) => {
                warn!(?lib_path, "failed to load signal bridge: {e}");
                None
            }
        }
    }
}

/// Ensure the signal bridge is loaded and wired into the key manager.
///
/// If a bridge is already loaded, this is a no-op. Otherwise attempts to
/// load the DLL and stores it in both `PchatState` and `KeyManager`.
/// If a saved signal state file exists in the identity directory, it is
/// imported into the freshly created bridge so that sender key sessions
/// survive across reconnects.
pub(crate) fn ensure_signal_bridge(pchat: &mut PchatState) {
    if pchat.signal_bridge.is_some() {
        return;
    }
    let bridge = load_signal_bridge(&pchat.own_cert_hash, None);
    if let Some(ref b) = bridge {
        pchat.key_manager.set_signal_bridge(Arc::clone(b));
        load_signal_state(pchat.identity_dir.as_deref(), b);
    }
    pchat.signal_bridge = bridge;
}

/// Save the signal bridge state to disk so it can be restored on reconnect.
///
/// Writes the exported JSON blob to `<identity_dir>/signal_state.json`.
/// Errors are logged but not propagated -- persistence is best-effort.
pub(crate) fn save_signal_state(pchat: &PchatState) {
    let Some(ref bridge) = pchat.signal_bridge else {
        debug!("no signal bridge loaded; skipping signal state save");
        return;
    };
    let Some(ref dir) = pchat.identity_dir else {
        debug!("no identity_dir set; skipping signal state save");
        return;
    };
    match bridge.export_state() {
        Ok(data) => {
            let path = dir.join(SIGNAL_STATE_FILE);
            if let Err(e) = std::fs::write(&path, &data) {
                warn!(?path, "failed to write signal state: {e}");
            } else {
                debug!(?path, bytes = data.len(), "saved signal bridge state");
            }
        }
        Err(e) => {
            warn!("failed to export signal bridge state: {e}");
        }
    }
}

/// Save the local message cache to disk (AES-256-GCM encrypted).
///
/// Errors are logged but not propagated -- persistence is best-effort.
pub(crate) fn save_local_cache(pchat: &PchatState) {
    if let Some(ref cache) = pchat.local_cache {
        if let Err(e) = cache.save() {
            warn!("failed to save local message cache: {e}");
        }
    }
}

/// Load a previously saved signal state from disk into the bridge.
fn load_signal_state(identity_dir: Option<&Path>, bridge: &SignalBridge) {
    let Some(dir) = identity_dir else {
        return;
    };
    let path = dir.join(SIGNAL_STATE_FILE);
    if !path.exists() {
        debug!(?path, "no saved signal state found");
        return;
    }
    match std::fs::read(&path) {
        Ok(data) => match bridge.import_state(&data) {
            Ok(()) => debug!(?path, "restored signal bridge state from disk"),
            Err(e) => warn!(?path, "failed to import signal state: {e}"),
        },
        Err(e) => warn!(?path, "failed to read signal state file: {e}"),
    }
}

// ---- Signal sender key distribution ---------------------------------

/// Create our sender key distribution for a channel and broadcast it
/// to all channel members via `PluginDataTransmission`.
///
/// Each member who receives the distribution can then decrypt our
/// future messages on that channel.
pub(crate) fn send_signal_distribution(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
) {
    let (handle, distribution, sessions) = {
        let Ok(mut state) = shared.lock() else { return };

        let Some(ref mut pchat) = state.pchat else { return };
        ensure_signal_bridge(pchat);
        let Some(ref bridge) = pchat.signal_bridge else {
            warn!(channel_id, "cannot send signal distribution: bridge not loaded");
            return;
        };

        let dist = match bridge.create_distribution(channel_id) {
            Ok(d) => d,
            Err(e) => {
                warn!(channel_id, "create_distribution failed: {e}");
                return;
            }
        };

        // Collect all other user sessions in this channel.
        let own_session = state.own_session.unwrap_or(0);
        let receiver_sessions: Vec<u32> = state
            .users
            .values()
            .filter(|u| u.channel_id == channel_id && u.session != own_session)
            .map(|u| u.session)
            .collect();

        (state.client_handle.clone(), dist, receiver_sessions)
    };

    if sessions.is_empty() {
        debug!(channel_id, "no peers in channel, skipping distribution send");
        return;
    }

    let Some(handle) = handle else { return };

    let data_id = mumble_protocol::persistent::DATA_ID_SIGNAL_SENDER_KEY.to_string();
    let _dist_task = tokio::spawn(async move {
        if let Err(e) = handle
            .send(command::SendPluginData {
                receiver_sessions: sessions,
                data: distribution,
                data_id,
            })
            .await
        {
            warn!(channel_id, "failed to send signal distribution: {e}");
        } else {
            debug!(channel_id, "sent signal sender key distribution");
        }
    });
}

/// Retry decrypting stashed `SignalV1` envelopes from `sender_hash`.
///
/// Called after successfully processing a sender key distribution.  Any
/// envelopes that still fail to decrypt are kept in the stash so they
/// can be retried when a future distribution arrives (the current
/// distribution may not cover messages encrypted at an earlier chain
/// iteration).
fn retry_stashed_signal_envelopes(
    state: &mut SharedState,
    sender_hash: &str,
    sender_channel: u32,
) -> usize {
    // Phase 1: drain matching envelopes, decrypt, cache (borrows state.pchat).
    let decoded: Vec<(String, u32, String, String)> = {
        let Some(pchat) = state.pchat.as_mut() else {
            return 0;
        };

        // Partition: matching envelopes vs. the rest.
        let mut remaining = Vec::new();
        let mut matched = Vec::new();
        for env in pchat.pending_signal_envelopes.drain(..) {
            if env.sender_hash == sender_hash && env.channel_id == sender_channel {
                matched.push(env);
            } else {
                remaining.push(env);
            }
        }
        pchat.pending_signal_envelopes = remaining;

        if matched.is_empty() {
            return 0;
        }

        debug!(
            sender = %sender_hash,
            channel_id = sender_channel,
            count = matched.len(),
            "retrying stashed signal envelopes after distribution"
        );

        let mut results = Vec::new();
        let mut still_pending = Vec::new();

        for env in matched {
            let decrypt_result = pchat
                .key_manager
                .decrypt_signal(&env.sender_hash, env.channel_id, &env.envelope_bytes);

            match decrypt_result {
                Ok(plaintext) => match pchat.codec.decode::<MessageEnvelope>(&plaintext) {
                    Ok(envelope) => {
                        cache_signal_message(
                            pchat,
                            CachedMessage {
                                message_id: env.message_id.clone(),
                                channel_id: env.channel_id,
                                timestamp: env.timestamp,
                                sender_hash: env.sender_hash.clone(),
                                sender_name: envelope.sender_name.clone(),
                                body: envelope.body.clone(),
                                is_own: false,
                            },
                        );
                        results.push((
                            env.message_id.clone(),
                            env.channel_id,
                            envelope.sender_name,
                            envelope.body,
                        ));
                    }
                    Err(e) => {
                        warn!(
                            message_id = %env.message_id,
                            "stashed envelope: failed to decode after decrypt: {e}"
                        );
                    }
                },
                Err(e) => {
                    warn!(
                        message_id = %env.message_id,
                        sender = %env.sender_hash,
                        "stashed envelope: still failed to decrypt, keeping stashed: {e}"
                    );
                    still_pending.push(env);
                }
            }
        }

        // Put back envelopes that still failed so they can be retried
        // on a future distribution.
        pchat.pending_signal_envelopes.extend(still_pending);

        results
    };
    // `state.pchat` borrow is now dropped.

    // Phase 2: replace placeholder messages in `state.messages`.
    let mut replaced_count = 0usize;
    for (message_id, channel_id, sender_name, body) in &decoded {
        if let Some(msgs) = state.messages.get_mut(channel_id) {
            if let Some(msg) = msgs
                .iter_mut()
                .find(|m| m.message_id.as_deref() == Some(message_id.as_str()))
            {
                msg.body.clone_from(body);
                msg.sender_name.clone_from(sender_name);
                replaced_count += 1;
            }
        }
    }

    if replaced_count > 0 {
        debug!(
            replaced_count,
            sender = %sender_hash,
            channel_id = sender_channel,
            "replaced placeholder messages with decrypted content"
        );
    }

    replaced_count
}

/// Process a received Signal sender key distribution from a peer.
///
/// Called when we receive a `PluginDataTransmission` with
/// `data_id == DATA_ID_SIGNAL_SENDER_KEY`.
/// Returns `true` if stashed envelopes were successfully decrypted and
/// placeholder messages replaced (caller should emit `state-changed`).
pub(crate) fn handle_signal_sender_key(
    shared: &Arc<Mutex<SharedState>>,
    sender_session: u32,
    data: &[u8],
) -> bool {
    let Ok(mut state) = shared.lock() else { return false };

    // Resolve sender's cert hash from their session.
    let sender_hash = state
        .users
        .get(&sender_session)
        .and_then(|u| u.hash.clone());
    let Some(sender_hash) = sender_hash else {
        warn!(sender_session, "signal sender key from unknown session");
        return false;
    };

    // Determine the sender's channel to use as the distribution channel_id.
    let sender_channel = state
        .users
        .get(&sender_session)
        .map(|u| u.channel_id)
        .unwrap_or(0);

    // Scope the mutable borrow of `state.pchat` so we can pass `&mut state`
    // to the retry helper afterwards.
    {
        let Some(ref mut pchat) = state.pchat else { return false };
        ensure_signal_bridge(pchat);
        let Some(ref bridge) = pchat.signal_bridge else {
            warn!("signal bridge not loaded, cannot process sender key");
            return false;
        };

        match bridge.process_distribution(&sender_hash, sender_channel, data) {
            Ok(()) => {
                debug!(
                    sender = %sender_hash,
                    channel_id = sender_channel,
                    "processed signal sender key distribution"
                );
            }
            Err(e) => {
                warn!(
                    sender = %sender_hash,
                    channel_id = sender_channel,
                    "failed to process signal distribution: {e}"
                );
                return false;
            }
        }
    }

    // After successfully processing the distribution, retry any stashed
    // envelopes from this sender on this channel.
    retry_stashed_signal_envelopes(&mut state, &sender_hash, sender_channel) > 0
}

/// Emit a `pchat-history-loading` event to the frontend via an `AppHandle`
/// stored in `SharedState`. Call with `loading: true` before starting
/// key-exchange wait / history fetch and `loading: false` when done.
pub(crate) fn emit_history_loading(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
    loading: bool,
) {
    use tauri::Emitter;
    use super::types::PchatHistoryLoadingPayload;

    let app = shared
        .lock()
        .ok()
        .and_then(|s| s.tauri_app_handle.clone());
    if let Some(app) = app {
        let _ = app.emit(
            "pchat-history-loading",
            PchatHistoryLoadingPayload { channel_id, loading },
        );
    }
}

// ---- Archive key persistence ----------------------------------------

/// File name for persisted archive keys inside the identity directory.
const ARCHIVE_KEYS_FILE: &str = "archive_keys.json";

/// On-disk representation of a single archive key.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedArchiveKey {
    /// 32-byte key encoded as 64-character hex string.
    key_hex: String,
    /// Cert hash of the key originator (who generated the key).
    originator: Option<String>,
}

/// Persist a single archive key to disk.
///
/// Reads the existing JSON file, upserts the entry for `channel_id`,
/// and writes back. Thread-safe via the file system (only one writer
/// expected at a time since we hold the `SharedState` mutex while
/// extracting the data).
pub(crate) fn persist_archive_key(
    identity_dir: &Path,
    channel_id: u32,
    key: &[u8; 32],
    originator: Option<&str>,
) {
    let path = identity_dir.join(ARCHIVE_KEYS_FILE);

    let mut keys: std::collections::HashMap<String, PersistedArchiveKey> =
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

    let key_hex: String = bytes_to_hex(key);
    let _ = keys.insert(
        channel_id.to_string(),
        PersistedArchiveKey {
            key_hex,
            originator: originator.map(String::from),
        },
    );

    match serde_json::to_string_pretty(&keys) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                warn!("failed to persist archive key: {e}");
            } else {
                debug!(channel_id, "persisted archive key to disk");
            }
        }
        Err(e) => warn!("failed to serialize archive keys: {e}"),
    }
}

/// Delete the persisted archive key for a single channel.
pub(crate) fn delete_persisted_archive_key(
    identity_dir: &Path,
    channel_id: u32,
) {
    let path = identity_dir.join(ARCHIVE_KEYS_FILE);

    let mut keys: std::collections::HashMap<String, PersistedArchiveKey> =
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

    if keys.remove(&channel_id.to_string()).is_some() {
        match serde_json::to_string_pretty(&keys) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    warn!("failed to update archive keys file: {e}");
                } else {
                    debug!(channel_id, "removed persisted archive key from disk");
                }
            }
            Err(e) => warn!("failed to serialize archive keys: {e}"),
        }
    }
}

/// Load all persisted archive keys from disk.
///
/// Returns `(channel_id, key_bytes, originator)` tuples. Entries with
/// invalid hex or wrong key length are silently skipped.
pub(crate) fn load_persisted_archive_keys(
    identity_dir: &Path,
) -> Vec<(u32, [u8; 32], Option<String>)> {
    let path = identity_dir.join(ARCHIVE_KEYS_FILE);

    let keys: std::collections::HashMap<String, PersistedArchiveKey> =
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

    keys.into_iter()
        .filter_map(|(ch_str, entry)| {
            let ch: u32 = ch_str.parse().ok()?;
            let key_bytes = hex_decode(&entry.key_hex)?;
            if key_bytes.len() != 32 {
                return None;
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&key_bytes);
            Some((ch, key, entry.originator))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn persist_and_load_archive_key_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let key: [u8; 32] = [42u8; 32];
        let originator = "abc123";

        persist_archive_key(dir.path(), 5, &key, Some(originator));

        let loaded = load_persisted_archive_keys(dir.path());
        assert_eq!(loaded.len(), 1);
        let (ch, loaded_key, loaded_orig) = &loaded[0];
        assert_eq!(*ch, 5);
        assert_eq!(*loaded_key, key);
        assert_eq!(loaded_orig.as_deref(), Some(originator));
    }

    #[test]
    fn persist_multiple_channels() {
        let dir = tempfile::tempdir().unwrap();
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];

        persist_archive_key(dir.path(), 1, &key1, Some("orig1"));
        persist_archive_key(dir.path(), 7, &key2, None);

        let mut loaded = load_persisted_archive_keys(dir.path());
        loaded.sort_by_key(|(ch, _, _)| *ch);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0], (1, key1, Some("orig1".to_string())));
        assert_eq!(loaded[1], (7, key2, None));
    }

    #[test]
    fn persist_overwrites_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let key_old = [10u8; 32];
        let key_new = [20u8; 32];

        persist_archive_key(dir.path(), 3, &key_old, Some("orig_old"));
        persist_archive_key(dir.path(), 3, &key_new, Some("orig_new"));

        let loaded = load_persisted_archive_keys(dir.path());
        assert_eq!(loaded.len(), 1);
        let (ch, key, orig) = &loaded[0];
        assert_eq!(*ch, 3);
        assert_eq!(*key, key_new);
        assert_eq!(orig.as_deref(), Some("orig_new"));
    }

    #[test]
    fn load_from_nonexistent_dir_returns_empty() {
        let dir = Path::new("/nonexistent/path/12345");
        let loaded = load_persisted_archive_keys(dir);
        assert!(loaded.is_empty());
    }

    #[test]
    fn load_ignores_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(ARCHIVE_KEYS_FILE), "not valid json").unwrap();
        let loaded = load_persisted_archive_keys(dir.path());
        assert!(loaded.is_empty());
    }

    #[test]
    fn load_ignores_wrong_key_length() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{"1": {"key_hex": "aabb", "originator": null}}"#;
        std::fs::write(dir.path().join(ARCHIVE_KEYS_FILE), json).unwrap();
        let loaded = load_persisted_archive_keys(dir.path());
        assert!(loaded.is_empty());
    }
}
