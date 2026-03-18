//! Persistent encrypted chat integration layer.
//!
//! Bridges `mumble-protocol`'s persistent chat primitives (`KeyManager`,
//! wire structs, encryption) to the Tauri application state. Handles
//! sending and receiving pchat messages using native protobuf message types.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{debug, info, warn};

use mumble_protocol::client::ClientHandle;
use mumble_protocol::command;
use mumble_protocol::persistent::keys::{KeyManager, SeedIdentity};
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
use mumble_protocol::persistent::PersistenceMode;
use mumble_protocol::proto::mumble_tcp;

use super::types::ChatMessage;
use super::SharedState;

/// Persistent chat manager — lives inside `SharedState`.
#[allow(dead_code)]
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
}

impl PchatState {
    /// Create a new pchat state from a 32-byte identity seed and our cert hash.
    pub fn new(seed: [u8; 32], own_cert_hash: String) -> Result<Self, String> {
        let identity = SeedIdentity::from_seed(&seed)
            .map_err(|e| format!("Failed to derive pchat identity: {e}"))?;
        let key_manager = KeyManager::new(Box::new(identity));
        let codec = MsgPackCodec;
        let volatile = VolatileMessageProvider::new();
        let backend = InMemoryPersistentBackend::new();
        let persistent = PersistentMessageProvider::new(Box::new(backend));
        let provider = CompositeMessageProvider::new(volatile, persistent);

        Ok(Self {
            key_manager,
            own_cert_hash,
            codec,
            seed,
            provider,
            fetched_channels: std::collections::HashSet::new(),
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

/// Legacy paths used before per-identity storage was introduced.
const LEGACY_PCHAT_DIR: &str = "pchat";
const LEGACY_SEED_FILE: &str = "identity_seed.bin";
const LEGACY_CERTS_DIR: &str = "certs";

/// Return the directory for a given identity label:
/// `<app_data>/identities/<label>/`
pub(crate) fn identity_dir(app_data_dir: &std::path::Path, label: &str) -> std::path::PathBuf {
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
pub(crate) fn migrate_legacy_storage(app_data_dir: &std::path::Path) {
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
    let entries = match std::fs::read_dir(&legacy_certs) {
        Ok(e) => e,
        Err(_) => return,
    };
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
    app_data_dir: &std::path::Path,
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
    app_data_dir: &std::path::Path,
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
    app_data_dir: &std::path::Path,
    label: &str,
) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let dir = identity_dir(app_data_dir, label);
    let cert = std::fs::read(dir.join(TLS_CERT_FILE)).ok();
    let key = std::fs::read(dir.join(TLS_KEY_FILE)).ok();
    (cert, key)
}

/// List all identity labels (subdirectories of `identities/`).
pub(crate) fn list_identity_labels(app_data_dir: &std::path::Path) -> Vec<String> {
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
pub(crate) fn delete_identity(app_data_dir: &std::path::Path, label: &str) -> Result<(), String> {
    let dir = identity_dir(app_data_dir, label);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---- Proto <-> Wire conversion helpers ------------------------------

fn persistence_mode_to_proto(mode: PersistenceMode) -> i32 {
    match mode {
        PersistenceMode::PostJoin => mumble_tcp::PchatPersistenceMode::PchatModePostJoin as i32,
        PersistenceMode::FullArchive => mumble_tcp::PchatPersistenceMode::PchatModeFullArchive as i32,
        _ => mumble_tcp::PchatPersistenceMode::PchatModePostJoin as i32,
    }
}

fn proto_to_persistence_mode(mode: Option<i32>) -> PersistenceMode {
    match mode.and_then(|v| mumble_tcp::PchatPersistenceMode::try_from(v).ok()) {
        Some(mumble_tcp::PchatPersistenceMode::PchatModePostJoin) => PersistenceMode::PostJoin,
        Some(mumble_tcp::PchatPersistenceMode::PchatModeFullArchive) => PersistenceMode::FullArchive,
        None => PersistenceMode::PostJoin,
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
        mode: proto_mode_to_wire_str(p.mode),
        requester_hash: p.requester_hash.clone().unwrap_or_default(),
        requester_public: p.requester_public.clone().unwrap_or_default(),
        request_id: p.request_id.clone().unwrap_or_default(),
        timestamp: p.timestamp.unwrap_or(0),
        relay_cap: p.relay_cap.unwrap_or(3),
    }
}

fn wire_key_exchange_to_proto(w: &WireKeyExchange) -> mumble_tcp::PchatKeyExchange {
    mumble_tcp::PchatKeyExchange {
        channel_id: Some(w.channel_id),
        mode: Some(wire_mode_str_to_proto(&w.mode)),
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
        mode: proto_mode_to_wire_str(p.mode),
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

fn proto_mode_to_wire_str(mode: Option<i32>) -> String {
    match proto_to_persistence_mode(mode) {
        PersistenceMode::PostJoin => "POST_JOIN".to_string(),
        PersistenceMode::FullArchive => "FULL_ARCHIVE".to_string(),
        _ => "POST_JOIN".to_string(),
    }
}

fn wire_mode_str_to_proto(s: &str) -> i32 {
    let mode = PersistenceMode::from_wire_str(s);
    persistence_mode_to_proto(mode)
}

// ---- Outbound: send key announce ------------------------------------

/// Send a key-announce to the server using native proto.
#[allow(dead_code)]
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

    info!(cert_hash, "sent pchat key-announce");
    Ok(())
}

// ---- Outbound: build encrypted plugin data (sync) ------------------

/// Encrypt a message and build a `PchatMessage` proto struct ready to send.
/// This is a synchronous operation (no network I/O) so it can be called
/// while holding the state lock.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_encrypted_pchat_message(
    pchat: &mut PchatState,
    channel_id: u32,
    mode: PersistenceMode,
    message_id: &str,
    body: &str,
    sender_name: &str,
    sender_session: u32,
    timestamp: u64,
) -> Result<mumble_tcp::PchatMessage, String> {
    debug!(
        channel_id,
        ?mode,
        message_id,
        timestamp,
        has_key = pchat.key_manager.has_key(channel_id, mode),
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
        .encrypt(mode, channel_id, message_id, timestamp, &envelope_bytes)
        .map_err(|e| format!("encrypt message: {e}"))?;

    Ok(mumble_tcp::PchatMessage {
        message_id: Some(message_id.to_string()),
        channel_id: Some(channel_id),
        timestamp: Some(timestamp),
        sender_hash: Some(pchat.own_cert_hash.clone()),
        mode: Some(persistence_mode_to_proto(mode)),
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
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn send_encrypted_message(
    handle: &ClientHandle,
    pchat: &mut PchatState,
    channel_id: u32,
    mode: PersistenceMode,
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
        .encrypt(mode, channel_id, message_id, now, &envelope_bytes)
        .map_err(|e| format!("encrypt message: {e}"))?;

    let proto_msg = mumble_tcp::PchatMessage {
        message_id: Some(message_id.to_string()),
        channel_id: Some(channel_id),
        timestamp: Some(now),
        sender_hash: Some(pchat.own_cert_hash.clone()),
        mode: Some(persistence_mode_to_proto(mode)),
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

    info!(message_id, channel_id, "sent encrypted pchat message");
    Ok(())
}

// ---- Outbound: send fetch request -----------------------------------

/// Send a `PchatFetch` proto to request stored messages.
#[allow(dead_code)]
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

    info!(channel_id, "sent pchat-fetch");
    Ok(())
}

// ---- Inbound: process incoming proto messages -----------------------

pub(crate) fn handle_proto_key_announce(shared: &Arc<Mutex<SharedState>>, msg: &mumble_tcp::PchatKeyAnnounce) {
    let wire = proto_to_wire_key_announce(msg);

    info!(
        cert_hash = %wire.cert_hash,
        algo = wire.algorithm_version,
        id_pub_len = wire.identity_public.len(),
        sign_pub_len = wire.signing_public.len(),
        sig_len = wire.signature.len(),
        timestamp = wire.timestamp,
        "received pchat key-announce"
    );

    let Ok(mut state) = shared.lock() else { return };

    let mut should_push_keys = false;
    let peer_cert_hash = wire.cert_hash.clone();

    if let Some(ref mut pchat) = state.pchat {
        match pchat.key_manager.record_peer_key(&wire) {
            Ok(true) => {
                info!(cert_hash = %wire.cert_hash, "recorded peer key");
                should_push_keys = true;
            }
            Ok(false) => debug!(cert_hash = %wire.cert_hash, "stale key-announce discarded"),
            Err(e) => warn!(cert_hash = %wire.cert_hash, "failed to record peer key: {e}"),
        }
    }

    // After successfully recording a peer's public key, proactively push our
    // FullArchive channel keys to them.  This unblocks decryption for peers
    // whose DB key-announce was previously corrupted (pre-hex-encoding-fix):
    // once the live announce arrives and we can authenticate them, we
    // immediately distribute our channel key without requiring a channel rejoin.
    if should_push_keys {
        let proactive = build_proactive_key_exchanges(&state, &peer_cert_hash);
        if !proactive.is_empty() {
            if let Some(handle) = state.client_handle.clone() {
                tokio::spawn(async move {
                    for exchange in proactive {
                        if let Err(e) = handle
                            .send(command::SendPchatKeyExchange { exchange })
                            .await
                        {
                            warn!(cert_hash = %peer_cert_hash, "proactive key push failed: {e}");
                        }
                    }
                });
            }
        }
    }
}

/// Build direct (no `request_id`) key exchanges for every FullArchive channel
/// that `peer_cert_hash` is currently in and for which we hold the key.
///
/// Sending these immediately after a successful `record_peer_key` call means
/// the peer receives our channel key as soon as we can authenticate them,
/// without needing a channel rejoin to trigger the server's pending-request flow.
fn build_proactive_key_exchanges(
    state: &SharedState,
    peer_cert_hash: &str,
) -> Vec<mumble_tcp::PchatKeyExchange> {
    let Some(ref pchat) = state.pchat else {
        return Vec::new();
    };

    // Find the channel(s) the peer is currently present in.
    let peer_channel_ids: Vec<u32> = state
        .users
        .values()
        .filter(|u| u.hash.as_deref() == Some(peer_cert_hash))
        .map(|u| u.channel_id)
        .collect();

    if peer_channel_ids.is_empty() {
        return Vec::new();
    }

    // Retrieve the peer's DH public key (just recorded above).
    let Some(peer_record) = pchat.key_manager.get_peer(peer_cert_hash) else {
        return Vec::new();
    };
    let peer_x25519 = peer_record.dh_public;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let mut exchanges = Vec::new();

    for ch_id in peer_channel_ids {
        let is_full_archive = state
            .channels
            .get(&ch_id)
            .and_then(|ch| ch.pchat_mode)
            .map(PersistenceMode::from)
            == Some(PersistenceMode::FullArchive);

        if !is_full_archive || !pchat.key_manager.has_key(ch_id, PersistenceMode::FullArchive) {
            continue;
        }

        match pchat.key_manager.distribute_key(
            ch_id,
            PersistenceMode::FullArchive,
            0,
            peer_cert_hash,
            &peer_x25519,
            None, // no request_id: direct acceptance on recipient side
            now_ms,
        ) {
            Ok(mut wire_exchange) => {
                wire_exchange.sender_hash = pchat.own_cert_hash.clone();
                exchanges.push(wire_key_exchange_to_proto(&wire_exchange));
                info!(
                    channel_id = ch_id,
                    recipient = %peer_cert_hash,
                    "queued proactive FullArchive key push"
                );
            }
            Err(e) => {
                warn!(channel_id = ch_id, "failed to build proactive key exchange: {e}");
            }
        }
    }

    exchanges
}

pub(crate) fn handle_proto_key_request(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatKeyRequest,
) {
    let wire_request = proto_to_wire_key_request(msg);

    info!(
        channel_id = wire_request.channel_id,
        requester = %wire_request.requester_hash,
        request_id = %wire_request.request_id,
        "received pchat key-request"
    );

    let (exchange_proto, handle) = {
        let Ok(mut state) = shared.lock() else { return };

        let Some(pchat) = state.pchat.as_mut() else { return };

        let wire_exchange = match pchat.key_manager.handle_key_request(
            &wire_request,
            &pchat.own_cert_hash,
        ) {
            Ok(Some(ex)) => ex,
            Ok(None) => {
                info!(channel_id = wire_request.channel_id, "no key to share for this channel");
                return;
            }
            Err(e) => {
                warn!(channel_id = wire_request.channel_id, "handle_key_request failed: {e}");
                return;
            }
        };

        let proto = wire_key_exchange_to_proto(&wire_exchange);
        (proto, state.client_handle.clone())
    };

    if let Some(handle) = handle {
        tokio::spawn(async move {
            if let Err(e) = handle
                .send(command::SendPchatKeyExchange { exchange: exchange_proto })
                .await
            {
                warn!("failed to send key-exchange response: {e}");
            } else {
                info!("sent pchat key-exchange response");
            }
        });
    }
}

pub(crate) fn handle_proto_key_exchange(shared: &Arc<Mutex<SharedState>>, msg: &mumble_tcp::PchatKeyExchange) {
    let wire_exchange = proto_to_wire_key_exchange(msg);

    info!(
        channel_id = wire_exchange.channel_id,
        sender = %wire_exchange.sender_hash,
        epoch = wire_exchange.epoch,
        "received pchat key-exchange"
    );

    let channel_id = wire_exchange.channel_id;
    let mode = PersistenceMode::from_wire_str(&wire_exchange.mode);
    let request_id = wire_exchange.request_id.clone();

    let Ok(mut state) = shared.lock() else { return };

    let mut key_accepted = false;

    if let Some(ref mut pchat) = state.pchat {
        match pchat.key_manager.receive_key_exchange(&wire_exchange, None) {
            Ok(()) => {
                info!(
                    channel_id = wire_exchange.channel_id,
                    epoch = wire_exchange.epoch,
                    "accepted key-exchange"
                );

                // For FullArchive, receive_key_exchange puts the key into
                // pending_consensus (when request_id is present). We must
                // evaluate consensus immediately to promote it into
                // archive_keys so that has_key() returns true.
                if mode == PersistenceMode::FullArchive {
                    if let Some(ref rid) = request_id {
                        match pchat.key_manager.evaluate_consensus(rid, channel_id, &[]) {
                            Ok((trust, Some(_key))) => {
                                info!(
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
                        key_accepted = pchat.key_manager.has_key(channel_id, mode);
                    }
                } else {
                    key_accepted = pchat.key_manager.has_key(channel_id, mode);
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
        retry_decrypt_pending_messages(&mut state, channel_id, mode);
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
    _mode: PersistenceMode,
) {
    let has_placeholders = state
        .messages
        .get(&channel_id)
        .is_some_and(|msgs| msgs.iter().any(|m| m.body == "[Encrypted message - awaiting key]"));

    if !has_placeholders {
        return;
    }

    info!(
        channel_id,
        "removing placeholder messages and re-fetching after key exchange"
    );

    // Remove placeholder messages so re-fetch can replace them.
    if let Some(msgs) = state.messages.get_mut(&channel_id) {
        msgs.retain(|m| m.body != "[Encrypted message - awaiting key]");
    }

    // Allow re-fetching this channel's history.
    if let Some(ref mut pchat) = state.pchat {
        pchat.fetched_channels.remove(&channel_id);
    }

    // Spawn an async re-fetch so the messages are pulled now with the correct key.
    let handle = state.client_handle.clone();
    if let Some(handle) = handle {
        tokio::spawn(async move {
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
                info!(channel_id, "sent pchat re-fetch after key exchange");
            }
        });
    }
}

pub(crate) fn handle_proto_msg_deliver(shared: &Arc<Mutex<SharedState>>, msg: &mumble_tcp::PchatMessageDeliver) {
    let message_id = msg.message_id.clone().unwrap_or_default();
    let channel_id = msg.channel_id.unwrap_or(0);
    let timestamp = msg.timestamp.unwrap_or(0);
    let sender_hash = msg.sender_hash.clone().unwrap_or_default();
    let mode = proto_to_persistence_mode(msg.mode);
    let envelope_bytes = msg.envelope.clone().unwrap_or_default();
    let replaces_id = msg.replaces_id.clone();

    debug!(data_len = envelope_bytes.len(), "pchat: handle_proto_msg_deliver entry");

    info!(
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

    // Build empty epoch_fingerprint if not provided
    let _epoch_fp: [u8; 8] = msg.replaces_id.as_ref().map(|_| [0u8; 8]).unwrap_or([0u8; 8]);
    // Actually we don't have epoch_fingerprint in PchatMessageDeliver — it's only in PchatMessage.
    // For decryption, we need epoch info. Since PchatMessageDeliver doesn't carry epoch/chain_index/epoch_fingerprint,
    // this is a broadcast notification — the server already stored it. We still need to decrypt though.
    // Looking at the proto definition, PchatMessageDeliver only has: message_id, channel_id, timestamp, sender_hash, mode, envelope, replaces_id.
    // We need to handle this with a "latest epoch" approach for decryption.

    // For PchatMessageDeliver (real-time), we try decrypting with the current epoch key.
    // The decrypt method needs an EncryptedPayload, but we don't have epoch/chain_index here.
    // Since this is a real-time delivery, we can try decrypt with epoch=None (latest).
    let payload = mumble_protocol::persistent::keys::EncryptedPayload {
        ciphertext: envelope_bytes,
        epoch: None,
        chain_index: None,
        epoch_fingerprint: [0u8; 8],
    };

    let (body, sender_name) = match pchat
        .key_manager
        .decrypt(mode, channel_id, &message_id, timestamp, &payload)
    {
        Ok(plaintext) => {
            debug!(message_id = %message_id, plaintext_len = plaintext.len(), "pchat msg-deliver: decrypted OK");
            match pchat.codec.decode::<MessageEnvelope>(&plaintext) {
                Ok(env) => (env.body, env.sender_name),
                Err(e) => {
                    warn!(message_id = %message_id, "failed to decode envelope: {e}");
                    return;
                }
            }
        }
        Err(e) => {
            warn!(message_id = %message_id, channel_id, has_key = pchat.key_manager.has_key(channel_id, mode), "failed to decrypt message: {e}");
            (
                "[Encrypted message - awaiting key]".to_string(),
                sender_hash.clone(),
            )
        }
    };

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

pub(crate) fn handle_proto_fetch_resp(shared: &Arc<Mutex<SharedState>>, msg: &mumble_tcp::PchatFetchResponse) {
    let channel_id = msg.channel_id.unwrap_or(0);
    let has_more = msg.has_more.unwrap_or(false);
    let total_stored = msg.total_stored.unwrap_or(0);

    debug!(data_len = msg.messages.len(), "pchat: handle_proto_fetch_resp entry");

    info!(
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
        let mode = proto_to_persistence_mode(proto_msg.mode);
        let has_key = pchat.key_manager.has_key(msg_channel_id, mode);

        debug!(
            message_id = %msg_id,
            channel_id = msg_channel_id,
            timestamp = msg_timestamp,
            sender = %msg_sender_hash,
            envelope_len = proto_msg.envelope.as_ref().map(Vec::len).unwrap_or(0),
            has_key,
            "pchat fetch-resp: processing message"
        );

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

        let (body, sender_name, _decrypted) = match pchat
            .key_manager
            .decrypt(mode, msg_channel_id, &msg_id, msg_timestamp, &payload)
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

        info!(
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
        });
    }

    if !decrypted_msgs.is_empty() {
        debug!(
            channel_id,
            new_count = decrypted_msgs.len(),
            "pchat fetch-resp: inserting decrypted messages"
        );
        let existing = state.messages.entry(channel_id).or_default();

        // De-duplicate: only add messages we don't already have
        let existing_ids: std::collections::HashSet<Option<&str>> = existing
            .iter()
            .map(|m| m.message_id.as_deref())
            .collect();

        let mut new_msgs: Vec<ChatMessage> = decrypted_msgs
            .into_iter()
            .filter(|m| !existing_ids.contains(&m.message_id.as_deref()))
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
    let message_id = msg.message_id.as_deref().unwrap_or("");
    let status = msg.status.unwrap_or(0);
    let reason = msg.reason.as_deref();

    if status == mumble_tcp::PchatAckStatus::PchatAckRejected as i32
        || status == mumble_tcp::PchatAckStatus::PchatAckQuotaExceeded as i32
    {
        warn!(
            message_id,
            status,
            reason = ?reason,
            "pchat message rejected by server"
        );
    } else {
        info!(
            message_id,
            status,
            "received pchat ack"
        );
    }
}

// ---- Helper ---------------------------------------------------------

pub(crate) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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
