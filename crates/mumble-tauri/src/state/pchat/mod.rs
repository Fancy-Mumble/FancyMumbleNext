//! Persistent encrypted chat integration layer.
//!
//! Bridges `mumble-protocol`'s persistent chat primitives (`KeyManager`,
//! wire structs, encryption) to the Tauri application state. Handles
//! sending and receiving pchat messages using native protobuf message types.
//!
//! # Module layout
//!
//! - `settings`      -- shared constants (file names, limits)
//! - `conversion`    -- proto <-> wire type conversions
//! - `identity`      -- `IdentityStore` for certificate/seed CRUD
//! - `persistence`   -- archive key disk persistence, signal/cache save
//! - `outbound`      -- outbound message construction and sending
//! - `inbound`       -- inbound message handlers (deliver, fetch, ack, etc.)
//! - `key_exchange`  -- key announce/request/exchange handlers
//! - `key_sharing`   -- key challenges, holder reporting, takeover
//! - `signal_bridge` -- Signal Protocol bridge loading and distribution

mod settings;
mod conversion;
pub(crate) mod identity;
mod persistence;
mod outbound;
mod inbound;
mod key_exchange;
mod key_sharing;
mod signal_bridge;

// -- Re-exports -------------------------------------------------------

// Conversion
pub(crate) use conversion::{wire_key_announce_to_proto, wire_key_exchange_to_proto};

// Identity
pub(crate) use identity::IdentityStore;

// Persistence
pub(crate) use persistence::{persist_archive_key, delete_persisted_archive_key, load_persisted_archive_keys};

// Outbound
pub(crate) use outbound::{send_fetch, OutboundMessage};

// Inbound
pub(crate) use inbound::{
    handle_proto_msg_deliver, handle_proto_fetch_resp, handle_proto_ack,
    handle_proto_delete_messages, handle_proto_offline_queue_drain,
};

// Key exchange
pub(crate) use key_exchange::{
    handle_proto_key_announce, handle_proto_key_request,
    handle_proto_key_exchange, check_key_share_for_channel,
};

// Key sharing
pub(crate) use key_sharing::{
    handle_proto_key_challenge, handle_proto_key_challenge_result,
    send_key_holder_report_async,
    send_key_takeover, query_key_holders,
};

// Signal bridge
pub(crate) use signal_bridge::{
    ensure_signal_bridge_unlocked,
    send_signal_distribution, handle_signal_sender_key,
};

// -- Core types -------------------------------------------------------

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::warn;

use mumble_protocol::persistent::keys::{EncryptedPayload, KeyManager, SeedIdentity};
use mumble_protocol::persistent::protocol::signal_v1::SignalBridge;
use mumble_protocol::persistent::wire::{MessageEnvelope, MsgPackCodec, WireCodec};
use mumble_protocol::persistent::PchatProtocol;

use super::local_cache::{CachedMessage, LocalMessageCache};
use super::types::{PchatHistoryLoadingPayload, SignalBridgeErrorPayload};
use super::SharedState;

use settings::MAX_STASHED_ENVELOPES;

/// Parameters for decrypting an inbound encrypted envelope.
pub(crate) struct InboundEnvelope<'a> {
    pub protocol: PchatProtocol,
    pub sender_hash: &'a str,
    pub channel_id: u32,
    pub message_id: &'a str,
    pub timestamp: u64,
    pub envelope_bytes: &'a [u8],
    pub epoch: Option<u32>,
    pub chain_index: Option<u32>,
    pub epoch_fingerprint: [u8; 8],
}

/// Persistent chat manager -- lives inside `SharedState`.
pub(crate) struct PchatState {
    /// Our E2EE key manager (identity + peer keys + epoch/archive keys).
    pub key_manager: KeyManager,
    /// Our TLS certificate hash (stable identity across sessions).
    pub own_cert_hash: String,
    /// `MessagePack` codec for wire serialization.
    pub codec: MsgPackCodec,
    /// Identity seed bytes (persisted to disk).
    pub seed: [u8; 32],
    /// Channels where we've already sent a fetch request (avoid duplicates).
    pub fetched_channels: std::collections::HashSet<u32>,
    /// Path to the per-identity storage directory (for persisting archive keys).
    pub identity_dir: Option<PathBuf>,
    /// Signal Protocol bridge (loaded from external DLL, AGPL-isolated).
    pub signal_bridge: Option<Arc<SignalBridge>>,
    /// Set to `true` after `load_signal_bridge` returns `None` so that
    /// subsequent calls to `ensure_signal_bridge` short-circuit without
    /// repeating the file-system search.
    pub signal_bridge_load_failed: bool,
    /// Encrypted local message cache for `SignalV1` channels.
    pub local_cache: Option<LocalMessageCache>,
    /// Stashed encrypted envelopes for `SignalV1` messages that arrived
    /// before the sender's distribution key.
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

impl InboundEnvelope<'_> {
    /// Decrypt and decode an encrypted message envelope.
    ///
    /// Works for both `FancyV1` and `SignalV1` protocols, dispatching to
    /// the appropriate decryption path inside `KeyManager`.
    pub(crate) fn decrypt(
        &self,
        pchat: &mut PchatState,
    ) -> Result<MessageEnvelope, String> {
        let plaintext = if self.protocol == PchatProtocol::SignalV1 {
            pchat.key_manager
                .decrypt_signal(self.sender_hash, self.channel_id, self.envelope_bytes)
                .map_err(|e| format!("{e}"))?
        } else {
            let payload = EncryptedPayload {
                ciphertext: self.envelope_bytes.to_vec(),
                epoch: self.epoch,
                chain_index: self.chain_index,
                epoch_fingerprint: self.epoch_fingerprint,
            };
            pchat.key_manager
                .decrypt(self.protocol, self.channel_id, self.message_id, self.timestamp, &payload)
                .map_err(|e| format!("{e}"))?
        };

        pchat.codec
            .decode::<MessageEnvelope>(&plaintext)
            .map_err(|e| format!("decode envelope: {e}"))
    }
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
            fetched_channels: std::collections::HashSet::new(),
            identity_dir,
            signal_bridge: None,
            signal_bridge_load_failed: false,
            local_cache,
            pending_signal_envelopes: Vec::new(),
        })
    }

    /// Insert a decrypted message into the local cache (for `SignalV1`).
    pub(crate) fn cache_signal_message(&mut self, msg: CachedMessage) {
        if let Some(ref mut cache) = self.local_cache {
            cache.insert(msg);
        }
    }

    /// Stash an undecryptable `SignalV1` envelope for later retry.
    pub(crate) fn stash_signal_envelope(&mut self, env: PendingSignalEnvelope) {
        if self.pending_signal_envelopes.len() >= MAX_STASHED_ENVELOPES {
            warn!(
                max = MAX_STASHED_ENVELOPES,
                "stashed envelope limit reached, dropping oldest"
            );
            let _ = self.pending_signal_envelopes.remove(0);
        }
        self.pending_signal_envelopes.push(env);
    }
}

// -- Utility functions ------------------------------------------------

/// Current time as milliseconds since the UNIX epoch.
pub(crate) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Emit a `pchat-history-loading` event to the frontend.
pub(crate) fn emit_history_loading(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
    loading: bool,
) {
    use tauri::Emitter;

    let app = shared
        .lock()
        .ok()
        .and_then(|s| s.tauri_app_handle.clone());
    if let Some(app) = app {
        let _ = app.emit(
            "pchat-history-loading",
            PchatHistoryLoadingPayload {
                channel_id,
                loading,
            },
        );
    }
}

/// Emit a `pchat-signal-bridge-error` event to the frontend.
pub(crate) fn emit_signal_bridge_error(
    shared: &Arc<Mutex<SharedState>>,
    message: &str,
) {
    use tauri::Emitter;

    let app = shared
        .lock()
        .ok()
        .and_then(|s| s.tauri_app_handle.clone());
    if let Some(app) = app {
        let _ = app.emit(
            "pchat-signal-bridge-error",
            SignalBridgeErrorPayload {
                message: message.to_string(),
            },
        );
    }
}
