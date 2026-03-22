//! Shared application state for the Tauri backend.
//!
//! Mirrors the architecture of the Dioxus `MumbleBackend` but uses
//! Tauri's event system instead of mpsc channels to push updates to
//! the React frontend.
//!
//! Split into sub-modules for manageability:
//!
//! - [`types`] - serializable value types, event payloads, config structs.
//! - [`connection`] - `connect()` / `disconnect()` lifecycle.
//! - [`audio`] - voice pipeline management (enable, mute, deafen, outbound loop).
//! - [`event_handler`] - `EventHandler` bridge from mumble-protocol to Tauri events.

mod audio;
mod connection;
mod event_handler;
mod handler;
pub(crate) mod hash_names;
pub mod offload;
pub(crate) mod pchat;
mod search;
pub mod types;

// Re-export everything that lib.rs needs.
pub use types::{
    AudioDevice, AudioSettings, ChannelEntry, ChatMessage, ConnectionStatus, DebugStats,
    GroupChat, SearchResult, ServerConfig, ServerInfo, UserEntry, VoiceState,
};

use std::collections::{HashMap, HashSet};
#[cfg(not(target_os = "android"))]
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::{AppHandle, Emitter};
use tracing::info;

use offload::OffloadStore;

#[cfg(not(target_os = "android"))]
use mumble_protocol::audio::pipeline::InboundPipeline;
use mumble_protocol::client::ClientHandle;
use mumble_protocol::command;
use mumble_protocol::persistent::PersistenceMode;
use mumble_protocol::state::PchatMode;

use types::*;

/// Parse a frontend pchat mode string into the protobuf i32 value.
fn parse_pchat_mode_str(s: &str) -> PchatMode {
    match s {
        "post_join" => PchatMode::PostJoin,
        "full_archive" => PchatMode::FullArchive,
        "server_managed" => PchatMode::ServerManaged,
        _ => PchatMode::None,
    }
}

// --- Shared interior state ----------------------------------------

#[derive(Default)]
pub(super) struct SharedState {
    pub status: ConnectionStatus,
    /// Monotonically increasing counter - bumped on every `connect()` call.
    /// Used to detect stale `on_disconnected` callbacks from orphaned tasks.
    pub connection_epoch: u64,
    pub client_handle: Option<ClientHandle>,
    /// `JoinHandle` for the connecting-phase task (the outer `tokio::spawn`
    /// in `connect()`).  Stored so `disconnect()` can abort it while the
    /// TCP handshake is still in progress before the event loop starts.
    pub connect_task_handle: Option<tokio::task::JoinHandle<()>>,
    /// `JoinHandle` for the event-loop task so we can await a clean shutdown.
    pub event_loop_handle: Option<tokio::task::JoinHandle<()>>,
    pub users: HashMap<u32, UserEntry>,
    pub channels: HashMap<u32, ChannelEntry>,
    /// `channel_id` -> messages
    pub messages: HashMap<u32, Vec<ChatMessage>>,
    /// Direct message storage: `other_session` -> messages (conversation thread).
    pub dm_messages: HashMap<u32, Vec<ChatMessage>>,
    pub own_session: Option<u32>,
    pub own_name: String,
    /// Whether we've received `ServerSync` (initial state is complete).
    pub synced: bool,
    /// The Fancy Mumble protocol version announced by the server.
    /// `None` for standard Mumble servers (no extensions).
    pub server_fancy_version: Option<u64>,
    /// Server version/identity information from the `Version` message.
    pub server_version_info: ServerVersionInfo,
    /// Host the client connected to (for display in server info panel).
    pub connected_host: String,
    /// Port the client connected to.
    pub connected_port: u16,
    /// Maximum allowed users (from `ServerConfig`).
    pub max_users: Option<u32>,
    /// Maximum bandwidth from `ServerSync`.
    pub max_bandwidth: Option<u32>,
    /// Whether the server supports Opus.
    pub opus: bool,
    /// Channels the user has permanently opted to listen to (via context menu).
    pub permanently_listened: HashSet<u32>,
    /// The channel currently selected in the UI (viewing chat).
    pub selected_channel: Option<u32>,
    /// The channel the user is physically in (joined).
    pub current_channel: Option<u32>,
    /// Unread message counts per channel.
    pub unread_counts: HashMap<u32, u32>,
    /// Unread DM counts per user session.
    pub dm_unread_counts: HashMap<u32, u32>,
    /// The user session whose DM chat is currently viewed (mutually exclusive with `selected_channel`).
    pub selected_dm_user: Option<u32>,
    /// Group chats keyed by group UUID.
    pub group_chats: HashMap<String, GroupChat>,
    /// Group message storage: `group_id` -> messages.
    pub group_messages: HashMap<String, Vec<ChatMessage>>,
    /// Unread message counts per group.
    pub group_unread_counts: HashMap<String, u32>,
    /// The group currently viewed (mutually exclusive with `selected_channel` / `selected_dm_user`).
    pub selected_group: Option<String>,
    /// Server-reported configuration limits.
    pub server_config: ServerConfig,
    /// Server welcome text (HTML) from `ServerSync`.
    pub welcome_text: Option<String>,
    /// Audio settings (device, gain, VAD threshold).
    pub audio_settings: AudioSettings,
    /// Whether voice calling is active (inactive = deaf+muted).
    pub voice_state: VoiceState,
    /// Encrypted temp-file store for offloaded heavy message content.
    pub offload_store: Option<OffloadStore>,
    /// Inbound audio pipeline (network -> speakers).  Desktop only.
    #[cfg(not(target_os = "android"))]
    pub inbound_pipeline: Option<InboundPipeline>,
    /// Handle to the outbound audio capture task (mic -> network).  Desktop only.
    #[cfg(not(target_os = "android"))]
    pub outbound_task_handle: Option<tokio::task::JoinHandle<()>>,
    /// Live input volume multiplier (f32 stored as u32 bits). Updated atomically
    /// so volume slider changes take effect without pipeline restart.
    #[cfg(not(target_os = "android"))]
    pub input_volume_handle: Option<Arc<AtomicU32>>,
    /// Live output volume multiplier (f32 stored as u32 bits).
    #[cfg(not(target_os = "android"))]
    pub output_volume_handle: Option<Arc<AtomicU32>>,
    /// Handle to the background mic-test task (emits amplitude events). Desktop only.
    #[cfg(not(target_os = "android"))]
    pub mic_test_handle: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Handle to the background latency-test task (sends periodic pings). Desktop only.
    #[cfg(not(target_os = "android"))]
    pub latency_test_handle: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Persistent encrypted chat state (identity, key manager, codec).
    /// Initialised on connect when a cert hash is available.
    pub pchat: Option<pchat::PchatState>,
    /// Pre-loaded identity seed for pchat, set during `connect()`.
    /// Consumed by the `ServerSync` handler to build `PchatState`.
    pub pchat_seed: Option<[u8; 32]>,
    /// Per-identity directory path for persisting pchat keys etc.
    /// Set during `connect()`, consumed by `ServerSync` handler.
    pub pchat_identity_dir: Option<std::path::PathBuf>,
    /// Set to `true` when the user explicitly triggers a disconnect.
    /// Cleared by `on_disconnected()` after reading.
    pub user_initiated_disconnect: bool,
    /// Pending key-share requests waiting for user approval.
    /// Keyed by `(channel_id, peer_cert_hash)` encoded as string "`channel_id:cert_hash`".
    pub pending_key_shares: Vec<PendingKeyShare>,
    /// Server-tracked key holders per channel: `channel_id -> entries`.
    pub key_holders: HashMap<u32, Vec<KeyHolderEntry>>,
    /// Resolves cert hashes to human-readable names (persisted across sessions).
    pub hash_name_resolver: Option<Arc<dyn hash_names::HashNameResolver>>,
    /// Pending oneshot sender for `delete_pchat_messages` to wait for the server
    /// `PchatAck`.  Set before sending the delete request, consumed by the
    /// `PchatAck` handler.
    pub pending_delete_ack: Option<tokio::sync::oneshot::Sender<DeleteAckResult>>,
    /// Permission bitmask from `ServerSync.permissions` (root channel).
    /// Used as a fallback for channels that never receive a dedicated
    /// `PermissionQuery` response (e.g. `SuperUser`: the server skips
    /// per-channel replies when `user_id == 0`).
    pub root_permissions: Option<u32>,
    /// Tauri app handle for emitting events from spawned async tasks
    /// (e.g. pchat key exchange / history loading notifications).
    pub tauri_app_handle: Option<AppHandle>,
    /// Whether native OS notifications are enabled (user preference).
    pub notifications_enabled: bool,
}

// --- Tauri-managed application state ------------------------------

/// Central state managed by Tauri and shared across all commands.
pub struct AppState {
    pub(super) inner: Arc<Mutex<SharedState>>,
    app_handle: Mutex<Option<AppHandle>>,
    start_time: Instant,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SharedState { notifications_enabled: true, ..Default::default() })),
            app_handle: Mutex::new(None),
            start_time: Instant::now(),
        }
    }

    /// Inject the Tauri `AppHandle` during setup.
    pub fn set_app_handle(&self, handle: AppHandle) {
        *self.app_handle.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
    }

    pub(super) fn app_handle(&self) -> Option<AppHandle> {
        self.app_handle.lock().ok().and_then(|h| h.clone())
    }

    /// Recompute `user_count` for every channel based on current users.
    fn refresh_user_counts(state: &mut SharedState) {
        for ch in state.channels.values_mut() {
            ch.user_count = 0;
        }
        for user in state.users.values() {
            if let Some(ch) = state.channels.get_mut(&user.channel_id) {
                ch.user_count += 1;
            }
        }
    }

    // -- Query methods ---------------------------------------------

    pub fn status(&self) -> ConnectionStatus {
        self.inner
            .lock()
            .map(|s| s.status)
            .unwrap_or(ConnectionStatus::Disconnected)
    }

    pub fn channels(&self) -> Vec<ChannelEntry> {
        self.inner
            .lock()
            .map(|mut s| {
                Self::refresh_user_counts(&mut s);
                let root_perms = s.root_permissions;
                let mut channels: Vec<_> = s.channels.values().cloned().collect();
                // Fill in channels that never received a dedicated
                // PermissionQuery response with the root permissions
                // fallback (e.g. SuperUser where the server skips
                // per-channel replies).
                if let Some(fallback) = root_perms {
                    for ch in &mut channels {
                        if ch.permissions.is_none() {
                            ch.permissions = Some(fallback);
                        }
                    }
                }
                channels.sort_by_key(|c| c.id);
                channels
            })
            .unwrap_or_default()
    }

    pub fn users(&self) -> Vec<UserEntry> {
        self.inner
            .lock()
            .map(|s| s.users.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn messages(&self, channel_id: u32) -> Vec<ChatMessage> {
        self.inner
            .lock()
            .map(|s| s.messages.get(&channel_id).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    /// Direct messages with a specific user, keyed by their session ID.
    pub fn dm_messages(&self, session: u32) -> Vec<ChatMessage> {
        self.inner
            .lock()
            .map(|s| s.dm_messages.get(&session).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    /// Return our own session ID (assigned by the server after connect).
    pub fn get_own_session(&self) -> Option<u32> {
        self.inner.lock().ok().and_then(|s| s.own_session)
    }

    // -- Content offloading ----------------------------------------

    /// Initialise the encrypted offload store (called once during setup).
    pub fn init_offload_store(&self) -> Result<(), String> {
        OffloadStore::cleanup_stale();
        let store = OffloadStore::new()?;
        if let Ok(mut state) = self.inner.lock() {
            state.offload_store = Some(store);
        }
        Ok(())
    }

    /// Encrypt a message body and write it to a temp file, replacing
    /// the in-memory body with a lightweight placeholder that includes
    /// the original content byte-length (used for skeleton sizing).
    ///
    /// `scope` is one of `"channel"`, `"dm"`, or `"group"`.
    /// `scope_id` is the channel ID, DM session, or group UUID.
    pub fn offload_message(
        &self,
        message_id: String,
        scope: String,
        scope_id: String,
    ) -> Result<(), String> {
        let mut state = self.inner.lock().map_err(|e| e.to_string())?;

        // Locate the message first and extract its body.
        let body = Self::find_message_body(&state, &scope, &scope_id, &message_id)?;

        // Already offloaded.
        if body.starts_with("<!-- OFFLOADED:") {
            return Ok(());
        }

        let content_len = body.len();

        // Encrypt and write to disk.
        let store = state
            .offload_store
            .as_mut()
            .ok_or("Offload store not initialised")?;
        store.store(&message_id, &body)?;

        // Replace the in-memory body with a placeholder.
        // Format: <!-- OFFLOADED:{id}:{byte_length} -->
        Self::set_message_body(
            &mut state,
            &scope,
            &scope_id,
            &message_id,
            format!("<!-- OFFLOADED:{message_id}:{content_len} -->"),
        );

        Ok(())
    }

    /// Decrypt an offloaded message body from its temp file and restore
    /// it in the in-memory message store.  Returns the restored body.
    pub fn load_offloaded_message(
        &self,
        message_id: String,
        scope: String,
        scope_id: String,
    ) -> Result<String, String> {
        let mut state = self.inner.lock().map_err(|e| e.to_string())?;

        // Decrypt from disk.
        let store = state
            .offload_store
            .as_mut()
            .ok_or("Offload store not initialised")?;
        let body = store.load(&message_id)?;

        // Remove the temp file now that it is restored in memory.
        store.remove(&message_id);

        // Restore the in-memory body.
        Self::set_message_body(&mut state, &scope, &scope_id, &message_id, body.clone());

        Ok(body)
    }

    /// Decrypt multiple offloaded messages in a single call.
    ///
    /// Returns a map of `message_id` to restored body.  Keys that fail
    /// to decrypt are silently omitted.
    pub fn load_offloaded_messages_batch(
        &self,
        message_ids: Vec<String>,
        scope: String,
        scope_id: String,
    ) -> Result<HashMap<String, String>, String> {
        let mut state = self.inner.lock().map_err(|e| e.to_string())?;

        let store = state
            .offload_store
            .as_mut()
            .ok_or("Offload store not initialised")?;

        let key_refs: Vec<&str> = message_ids.iter().map(String::as_str).collect();
        let results = store.load_many(&key_refs);

        let mut restored = HashMap::new();
        for (key, result) in &results {
            if let Ok(body) = result {
                store.remove(key);
                let _ = restored.insert(key.clone(), body.clone());
            }
        }

        // Restore all successfully decrypted bodies in-memory.
        for (key, body) in &restored {
            Self::set_message_body(&mut state, &scope, &scope_id, key, body.clone());
        }

        Ok(restored)
    }

    /// Look up a message body across the channel / DM / group stores.
    fn find_message_body(
        state: &SharedState,
        scope: &str,
        scope_id: &str,
        message_id: &str,
    ) -> Result<String, String> {
        let messages = match scope {
            "channel" => {
                let ch_id: u32 = scope_id.parse().map_err(|_| "Invalid channel ID")?;
                state.messages.get(&ch_id)
            }
            "dm" => {
                let session: u32 = scope_id.parse().map_err(|_| "Invalid DM session")?;
                state.dm_messages.get(&session)
            }
            "group" => state.group_messages.get(scope_id),
            _ => return Err(format!("Unknown scope: {scope}")),
        };
        let messages = messages.ok_or("No messages found for scope")?;
        let msg = messages
            .iter()
            .find(|m| m.message_id.as_deref() == Some(message_id))
            .ok_or("Message not found")?;
        Ok(msg.body.clone())
    }

    /// Set a message body in the channel / DM / group stores.
    fn set_message_body(
        state: &mut SharedState,
        scope: &str,
        scope_id: &str,
        message_id: &str,
        body: String,
    ) {
        let messages = match scope {
            "channel" => {
                let ch_id: u32 = scope_id.parse().unwrap_or(0);
                state.messages.get_mut(&ch_id)
            }
            "dm" => {
                let session: u32 = scope_id.parse().unwrap_or(0);
                state.dm_messages.get_mut(&session)
            }
            "group" => state.group_messages.get_mut(scope_id),
            _ => None,
        };
        if let Some(messages) = messages {
            if let Some(msg) = messages
                .iter_mut()
                .find(|m| m.message_id.as_deref() == Some(message_id))
            {
                msg.body = body;
            }
        }
    }

    /// Delete all offloaded temp files and clear tracking.
    pub fn clear_offloaded(&self) {
        if let Ok(mut state) = self.inner.lock() {
            if let Some(store) = state.offload_store.as_mut() {
                store.clear();
            }
        }
    }

    /// Shut down the offload store, deleting the temp directory.
    pub fn shutdown_offload_store(&self) {
        if let Ok(mut state) = self.inner.lock() {
            if let Some(store) = state.offload_store.as_mut() {
                store.cleanup_dir();
            }
        }
    }

    // -- Messaging -------------------------------------------------

    /// Send a `PchatFetch` request to load older messages (pagination).
    pub async fn fetch_older_messages(
        &self,
        channel_id: u32,
        before_id: Option<String>,
        limit: u32,
    ) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        let handle = handle.ok_or("Not connected")?;
        pchat::send_fetch(&handle, channel_id, before_id, limit).await
    }

    #[allow(clippy::too_many_lines, reason = "message send path covers legacy text, fancy extensions, pchat encryption, and local storage")]
    pub async fn send_message(&self, channel_id: u32, body: String) -> Result<(), String> {
        let (handle, own_session, own_name, is_fancy, pchat_mode) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            let mode = state
                .channels
                .get(&channel_id)
                .and_then(|ch| ch.pchat_mode);
            (
                state.client_handle.clone(),
                state.own_session,
                state.own_name.clone(),
                state.server_fancy_version.is_some(),
                mode,
            )
        };

        let handle = handle.ok_or("Not connected")?;

        // Generate message_id and timestamp only when the server supports
        // Fancy Mumble extensions.  Legacy servers ignore unknown fields.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let message_id = if is_fancy {
            Some(uuid::Uuid::new_v4().to_string())
        } else {
            None
        };
        let timestamp = if is_fancy { Some(now_ms) } else { None };

        // Always send the plain TextMessage (backward compat / real-time path).
        handle
            .send(command::SendTextMessage {
                channel_ids: vec![channel_id],
                user_sessions: vec![],
                tree_ids: vec![],
                message: body.clone(),
                message_id: message_id.clone(),
                timestamp,
            })
            .await
            .map_err(|e| format!("Failed to send message: {e}"))?;

        // If the channel has persistent chat enabled, also send the encrypted
        // PchatMessage proto for server storage (dual-path per spec section 7.1).
        let persistence_mode = pchat_mode.map(PersistenceMode::from);
        tracing::debug!(
            channel_id,
            ?pchat_mode,
            ?persistence_mode,
            ?message_id,
            now_ms,
            "send_message: checking pchat path"
        );
        if let Some(mode) = persistence_mode {
            if mode.is_encrypted() {
                if let Some(ref msg_id) = message_id {
                    let session = own_session.unwrap_or(0);
                    // Build encrypted payload inside the lock, then send outside.
                    let send_result = {
                        let mut state = self.inner.lock().map_err(|e| e.to_string())?;
                        let client = state.client_handle.clone();
                        if let (Some(ref mut pchat_state), Some(client)) =
                            (&mut state.pchat, client)
                        {
                            match pchat::build_encrypted_pchat_message(
                                pchat_state,
                                channel_id,
                                mode,
                                msg_id,
                                &body,
                                &own_name,
                                session,
                                now_ms,
                            ) {
                                Ok(proto_msg) => Some((proto_msg, client)),
                                Err(e) => {
                                    tracing::warn!("pchat encrypt failed: {e}");
                                    None
                                }
                            }
                        } else {
                            None
                        }
                    };
                    // Send outside the lock
                    if let Some((proto_msg, client)) = send_result {
                        if let Err(e) = client
                            .send(command::SendPchatMessage { message: proto_msg })
                            .await
                        {
                            tracing::warn!("send pchat-msg failed: {e}");
                        }
                    }
                }
            }
        }

        // Add locally - the server does not echo our own messages back.
        if let Ok(mut state) = self.inner.lock() {
            let mut msg = ChatMessage {
                sender_session: own_session,
                sender_name: own_name,
                body,
                channel_id,
                is_own: true,
                dm_session: None,
                group_id: None,
                message_id,
                timestamp,
                is_legacy: false,
            };
            msg.ensure_id();
            state.messages.entry(channel_id).or_default().push(msg);
        }

        Ok(())
    }

    /// Send a direct message (DM) to a specific user by session ID.
    pub async fn send_dm(&self, target_session: u32, body: String) -> Result<(), String> {
        let (handle, own_session, own_name, is_fancy) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            (
                state.client_handle.clone(),
                state.own_session,
                state.own_name.clone(),
                state.server_fancy_version.is_some(),
            )
        };

        let handle = handle.ok_or("Not connected")?;

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let message_id = if is_fancy {
            Some(uuid::Uuid::new_v4().to_string())
        } else {
            None
        };
        let timestamp = if is_fancy { Some(now_ms) } else { None };

        handle
            .send(command::SendTextMessage {
                channel_ids: vec![],
                user_sessions: vec![target_session],
                tree_ids: vec![],
                message: body.clone(),
                message_id: message_id.clone(),
                timestamp,
            })
            .await
            .map_err(|e| format!("Failed to send DM: {e}"))?;

        // Store locally keyed by the target user's session.
        if let Ok(mut state) = self.inner.lock() {
            let mut msg = ChatMessage {
                sender_session: own_session,
                sender_name: own_name,
                body,
                channel_id: 0,
                is_own: true,
                dm_session: Some(target_session),
                group_id: None,
                message_id,
                timestamp,
                is_legacy: false,
            };
            msg.ensure_id();
            state.dm_messages.entry(target_session).or_default().push(msg);
        }

        Ok(())
    }

    // -- Plugin data ------------------------------------------------

    /// Send a plugin data transmission to the server.
    pub async fn send_plugin_data(
        &self,
        receiver_sessions: Vec<u32>,
        data: Vec<u8>,
        data_id: String,
    ) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        handle
            .send(command::SendPluginData {
                receiver_sessions,
                data,
                data_id,
            })
            .await
            .map_err(|e| format!("Failed to send plugin data: {e}"))?;

        Ok(())
    }

    // -- Pchat delete -----------------------------------------------

    /// Request deletion of persisted chat messages on the server.
    ///
    /// Blocks until the server acknowledges the request (or times out).
    /// Returns `Ok(())` on success, or `Err` with the server's rejection
    /// reason if the deletion was denied (e.g. missing `DeleteMessage` ACL).
    pub async fn delete_pchat_messages(
        &self,
        channel_id: u32,
        message_ids: Vec<String>,
        time_from: Option<u64>,
        time_to: Option<u64>,
        sender_hash: Option<String>,
    ) -> Result<(), String> {
        let (handle, rx) = {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            let h = state.client_handle.clone().ok_or("Not connected")?;

            // Install a oneshot so the PchatAck handler can send us the result.
            let (tx, rx) = tokio::sync::oneshot::channel::<DeleteAckResult>();
            state.pending_delete_ack = Some(tx);
            (h, rx)
        };

        let time_range = if time_from.is_some() || time_to.is_some() {
            Some(mumble_protocol::proto::mumble_tcp::pchat_delete_messages::TimeRange {
                from: time_from,
                to: time_to,
            })
        } else {
            None
        };

        handle
            .send(command::SendPchatDeleteMessages {
                message: mumble_protocol::proto::mumble_tcp::PchatDeleteMessages {
                    channel_id: Some(channel_id),
                    message_ids,
                    time_range,
                    sender_hash,
                },
            })
            .await
            .map_err(|e| format!("Failed to send pchat delete: {e}"))?;

        // Wait for the server's PchatAck (with a generous timeout).
        match tokio::time::timeout(std::time::Duration::from_secs(15), rx).await {
            Ok(Ok(ack)) if ack.success => Ok(()),
            Ok(Ok(ack)) => Err(format!(
                "Server rejected deletion: {}",
                ack.reason.unwrap_or_else(|| "permission denied".to_string())
            )),
            Ok(Err(_)) => Err("Delete acknowledgement channel closed".to_string()),
            Err(_) => Err("Delete request timed out".to_string()),
        }
    }

    // -- Channel browse / join / listen ------------------------------

    /// Select a channel in the UI for viewing (does NOT join it).
    /// Clears any active DM or group view.
    pub async fn select_channel(&self, channel_id: u32) -> Result<(), String> {
        let handle = {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.selected_channel = Some(channel_id);
            state.selected_dm_user = None;
            state.selected_group = None;
            // Mark the channel as read.
            let _ = state.unread_counts.remove(&channel_id);
            state.client_handle.clone()
        };
        self.emit_unreads();

        // Request fresh permissions so the UI can gate actions correctly.
        if let Some(handle) = handle {
            let _ = handle
                .send(command::PermissionQuery { channel_id })
                .await;
        }

        Ok(())
    }

    /// Actually move the user to a channel (join it on the server).
    pub async fn join_channel(&self, channel_id: u32) -> Result<(), String> {
        let handle = {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.current_channel = Some(channel_id);
            state.client_handle.clone()
        };

        if let Some(handle) = handle {
            let _ = handle
                .send(command::JoinChannel { channel_id })
                .await;
            // Request fresh permissions for the joined channel.
            let _ = handle
                .send(command::PermissionQuery { channel_id })
                .await;
        }

        // Emit current channel update to frontend.
        if let Some(app) = self.app_handle() {
            let _ = app.emit("current-channel-changed", CurrentChannelPayload { channel_id });
        }

        Ok(())
    }

    /// Get the channel the user is currently physically in.
    pub fn current_channel(&self) -> Option<u32> {
        self.inner
            .lock()
            .ok()
            .and_then(|s| s.current_channel)
    }

    /// Toggle permanent listening on a channel.
    ///
    /// Returns `Err` if the user lacks the Listen permission (`0x800`)
    /// on the target channel.
    pub async fn toggle_listen(&self, channel_id: u32) -> Result<bool, String> {
        info!(channel_id, "toggle_listen called");
        let (handle, is_now_listened, add, remove) = {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            let handle = state.client_handle.clone();

            // Check cached permissions before attempting to listen.
            // If permissions have been fetched and the Listen bit (0x800)
            // is NOT set, reject the request immediately.
            if !state.permanently_listened.contains(&channel_id) {
                if let Some(ch) = state.channels.get(&channel_id) {
                    if let Some(perms) = ch.permissions {
                        const LISTEN_BIT: u32 = 0x800;
                        if perms & LISTEN_BIT == 0 {
                            return Err(
                                "You do not have permission to listen to this channel".into(),
                            );
                        }
                    }
                }
            }

            if state.permanently_listened.contains(&channel_id) {
                // Unlisten - but keep listening if it's the selected channel.
                let _ = state.permanently_listened.remove(&channel_id);
                let is_selected = state.selected_channel == Some(channel_id);
                if is_selected {
                    // Still auto-listened, don't remove from server.
                    (handle, false, vec![], vec![])
                } else {
                    (handle, false, vec![], vec![channel_id])
                }
            } else {
                // Start permanent listen.
                let _ = state.permanently_listened.insert(channel_id);
                // If not already selected (and thus already listened), add.
                let is_selected = state.selected_channel == Some(channel_id);
                if is_selected {
                    (handle, true, vec![], vec![])
                } else {
                    (handle, true, vec![channel_id], vec![])
                }
            }
        };

        if let Some(handle) = handle {
            if !add.is_empty() || !remove.is_empty() {
                info!(?add, ?remove, is_now_listened, "sending ChannelListen");
                if let Err(e) = handle
                    .send(command::ChannelListen {
                        add: add.clone(),
                        remove: remove.clone(),
                    })
                    .await
                {
                    tracing::error!("failed to send ChannelListen: {e}");
                }
            } else {
                info!(
                    is_now_listened,
                    "toggle_listen: no protocol message needed (channel already listened via selection)"
                );
            }
        } else {
            tracing::warn!("toggle_listen: no client handle - not connected?");
        }

        Ok(is_now_listened)
    }

    /// Get the set of permanently listened channel IDs.
    pub fn listened_channels(&self) -> Vec<u32> {
        self.inner
            .lock()
            .map(|s| s.permanently_listened.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Get unread message counts per channel.
    pub fn unread_counts(&self) -> HashMap<u32, u32> {
        self.inner
            .lock()
            .map(|s| s.unread_counts.clone())
            .unwrap_or_default()
    }

    /// Mark a channel as read (clear unread count).
    pub fn mark_read(&self, channel_id: u32) {
        if let Ok(mut state) = self.inner.lock() {
            let _ = state.unread_counts.remove(&channel_id);
        }
        self.emit_unreads();
    }

    /// Emit the current unread counts to the frontend.
    fn emit_unreads(&self) {
        if let Some(handle) = self.app_handle() {
            let unreads = self.unread_counts();
            let _ = handle.emit("unread-changed", UnreadPayload { unreads });
        }
    }

    // -- Direct message helpers -------------------------------------

    /// Select a DM conversation for viewing. Clears the channel and group selection.
    pub fn select_dm_user(&self, session: u32) -> Result<(), String> {
        {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.selected_dm_user = Some(session);
            state.selected_channel = None;
            state.selected_group = None;
            // Mark DMs with this user as read.
            let _ = state.dm_unread_counts.remove(&session);
        }
        self.emit_dm_unreads();
        Ok(())
    }

    /// Get DM unread counts per user session.
    pub fn dm_unread_counts(&self) -> HashMap<u32, u32> {
        self.inner
            .lock()
            .map(|s| s.dm_unread_counts.clone())
            .unwrap_or_default()
    }

    /// Mark DMs with a specific user as read.
    pub fn mark_dm_read(&self, session: u32) {
        if let Ok(mut state) = self.inner.lock() {
            let _ = state.dm_unread_counts.remove(&session);
        }
        self.emit_dm_unreads();
    }

    /// Emit DM unread counts to the frontend.
    fn emit_dm_unreads(&self) {
        if let Some(handle) = self.app_handle() {
            let unreads = self.dm_unread_counts();
            let _ = handle.emit("dm-unread-changed", DmUnreadPayload { unreads });
        }
    }

    // -- Group chat helpers -----------------------------------------

    /// Create a new group chat and announce it to all members via plugin data.
    pub async fn create_group(
        &self,
        name: String,
        member_sessions: Vec<u32>,
    ) -> Result<GroupChat, String> {
        let (own_session, full_members) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            let own = state.own_session.ok_or("Not connected")?;
            // Ensure creator is in the member list.
            let mut members = member_sessions;
            if !members.contains(&own) {
                members.insert(0, own);
            }
            (own, members)
        };

        let group = GroupChat {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            members: full_members.clone(),
            creator: own_session,
        };

        // Store locally.
        if let Ok(mut state) = self.inner.lock() {
            let _ = state.group_chats.insert(group.id.clone(), group.clone());
        }

        // Announce to other members via plugin data.
        let other_members: Vec<u32> = full_members
            .iter()
            .copied()
            .filter(|&s| s != own_session)
            .collect();

        if !other_members.is_empty() {
            let payload = serde_json::json!({
                "action": "create",
                "group": group,
            });
            let data = payload.to_string().into_bytes();
            self.send_plugin_data(other_members, data, "fancy-group".into())
                .await?;
        }

        // Emit to frontend.
        if let Some(app) = self.app_handle() {
            let _ = app.emit("group-created", GroupCreatedPayload { group: group.clone() });
        }

        Ok(group)
    }

    /// Get all known group chats.
    pub fn groups(&self) -> Vec<GroupChat> {
        self.inner
            .lock()
            .map(|s| s.group_chats.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get messages for a specific group chat.
    pub fn group_messages(&self, group_id: &str) -> Vec<ChatMessage> {
        self.inner
            .lock()
            .map(|s| s.group_messages.get(group_id).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    /// Select a group chat for viewing. Clears channel and DM selection.
    pub fn select_group(&self, group_id: String) -> Result<(), String> {
        {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.selected_group = Some(group_id.clone());
            state.selected_channel = None;
            state.selected_dm_user = None;
            let _ = state.group_unread_counts.remove(&group_id);
        }
        self.emit_group_unreads();
        Ok(())
    }

    /// Get group unread counts.
    pub fn group_unread_counts(&self) -> HashMap<String, u32> {
        self.inner
            .lock()
            .map(|s| s.group_unread_counts.clone())
            .unwrap_or_default()
    }

    /// Mark a group chat as read.
    pub fn mark_group_read(&self, group_id: &str) {
        if let Ok(mut state) = self.inner.lock() {
            let _ = state.group_unread_counts.remove(group_id);
        }
        self.emit_group_unreads();
    }

    /// Emit group unread counts to the frontend.
    fn emit_group_unreads(&self) {
        if let Some(handle) = self.app_handle() {
            let unreads = self.group_unread_counts();
            let _ = handle.emit("group-unread-changed", GroupUnreadPayload { unreads });
        }
    }

    /// Send a message to a group chat.
    ///
    /// The message is sent as a `TextMessage` with `user_sessions` targeting
    /// all other group members.  The body is prefixed with a
    /// `<!-- FANCY_GROUP:group_id -->` marker so recipients can route it
    /// to the correct group conversation.
    pub async fn send_group_message(
        &self,
        group_id: String,
        body: String,
    ) -> Result<(), String> {
        let (handle, own_session, own_name, is_fancy, targets) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            let group = state
                .group_chats
                .get(&group_id)
                .ok_or("Group not found")?;
            let own = state.own_session.ok_or("Not connected")?;
            let targets: Vec<u32> = group
                .members
                .iter()
                .copied()
                .filter(|&s| s != own)
                .collect();
            (
                state.client_handle.clone(),
                Some(own),
                state.own_name.clone(),
                state.server_fancy_version.is_some(),
                targets,
            )
        };

        let handle = handle.ok_or("Not connected")?;

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let message_id = if is_fancy {
            Some(uuid::Uuid::new_v4().to_string())
        } else {
            None
        };
        let timestamp = if is_fancy { Some(now_ms) } else { None };

        // Prefix the body with the group marker.
        let wire_body = format!("<!-- FANCY_GROUP:{group_id} -->{body}");

        handle
            .send(command::SendTextMessage {
                channel_ids: vec![],
                user_sessions: targets,
                tree_ids: vec![],
                message: wire_body,
                message_id: message_id.clone(),
                timestamp,
            })
            .await
            .map_err(|e| format!("Failed to send group message: {e}"))?;

        // Store locally (without the marker prefix).
        if let Ok(mut state) = self.inner.lock() {
            let mut msg = ChatMessage {
                sender_session: own_session,
                sender_name: own_name,
                body,
                channel_id: 0,
                is_own: true,
                dm_session: None,
                group_id: Some(group_id),
                message_id,
                timestamp,
                is_legacy: false,
            };
            msg.ensure_id();
            state
                .group_messages
                .entry(msg.group_id.clone().unwrap_or_default())
                .or_default()
                .push(msg);
        }

        Ok(())
    }

    /// Get the server-reported config limits.
    pub fn server_config(&self) -> ServerConfig {
        self.inner
            .lock()
            .map(|s| s.server_config.clone())
            .unwrap_or_default()
    }

    /// Assemble a full server info snapshot for the frontend.
    pub fn server_info(&self) -> ServerInfo {
        self.inner
            .lock()
            .map(|s| {
                let vi = &s.server_version_info;
                // Format protocol version from the v2 or v1 encoding.
                let protocol_version = vi.version_v2.map(|v| {
                    let major = (v >> 48) & 0xFFFF;
                    let minor = (v >> 32) & 0xFFFF;
                    let patch = (v >> 16) & 0xFFFF;
                    format!("{major}.{minor}.{patch}")
                }).or_else(|| vi.version_v1.map(|v| {
                    let major = (v >> 16) & 0xFF;
                    let minor = (v >> 8) & 0xFF;
                    let patch = v & 0xFF;
                    format!("{major}.{minor}.{patch}")
                }));

                // Combine os + os_version into a single readable string:
                // "Linux" + "Ubuntu 24.04.1 LTS [arm64]" -> "Linux (Ubuntu 24.04.1 LTS [arm64])"
                let os = match (vi.os.as_deref(), vi.os_version.as_deref()) {
                    (Some(name), Some(ver)) if !ver.is_empty() => Some(format!("{name} ({ver})")),
                    (Some(name), _) => Some(name.to_owned()),
                    _ => None,
                };

                ServerInfo {
                    host: s.connected_host.clone(),
                    port: s.connected_port,
                    user_count: s.users.len() as u32,
                    max_users: s.max_users,
                    protocol_version,
                    fancy_version: s.server_fancy_version,
                    release: vi.release.clone(),
                    os,
                    max_bandwidth: s.max_bandwidth,
                    opus: s.opus,
                }
            })
            .unwrap_or_else(|_| ServerInfo {
                host: String::new(),
                port: 0,
                user_count: 0,
                max_users: None,
                protocol_version: None,
                fancy_version: None,
                release: None,
                os: None,
                max_bandwidth: None,
                opus: false,
            })
    }

    /// Collect debug statistics about the current application state.
    pub fn debug_stats(&self) -> DebugStats {
        self.inner
            .lock()
            .map(|s| {
                let channel_msgs: usize = s.messages.values().map(Vec::len).sum();
                let dm_msgs: usize = s.dm_messages.values().map(Vec::len).sum();
                let group_msgs: usize = s.group_messages.values().map(Vec::len).sum();
                let offloaded = s
                    .offload_store
                    .as_ref()
                    .map_or(0, OffloadStore::offloaded_count);

                DebugStats {
                    channel_message_count: channel_msgs,
                    dm_message_count: dm_msgs,
                    group_message_count: group_msgs,
                    total_message_count: channel_msgs + dm_msgs + group_msgs,
                    offloaded_count: offloaded,
                    channel_count: s.channels.len(),
                    user_count: s.users.len(),
                    group_count: s.group_chats.len(),
                    connection_epoch: s.connection_epoch,
                    voice_state: format!("{:?}", s.voice_state),
                    uptime_seconds: self.start_time.elapsed().as_secs(),
                }
            })
            .unwrap_or(DebugStats {
                channel_message_count: 0,
                dm_message_count: 0,
                group_message_count: 0,
                total_message_count: 0,
                offloaded_count: 0,
                channel_count: 0,
                user_count: 0,
                group_count: 0,
                connection_epoch: 0,
                voice_state: "Unknown".into(),
                uptime_seconds: self.start_time.elapsed().as_secs(),
            })
    }

    // -- Channel info ----------------------------------------------

    /// Return the server welcome text (from `ServerSync`), if any.
    pub fn welcome_text(&self) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|s| s.welcome_text.clone())
    }

    /// Update a channel on the server.
    ///
    /// All optional fields: only `Some(...)` values are sent.
    #[allow(clippy::too_many_arguments, reason = "channel update mirrors the full server-side parameter surface as optional fields")]
    pub async fn update_channel(
        &self,
        channel_id: u32,
        name: Option<String>,
        description: Option<String>,
        position: Option<i32>,
        temporary: Option<bool>,
        max_users: Option<u32>,
        pchat_mode: Option<String>,
        pchat_max_history: Option<u32>,
        pchat_retention_days: Option<u32>,
    ) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => {
                h.send(command::SetChannelState {
                    channel_id: Some(channel_id),
                    parent: None,
                    name,
                    description,
                    position,
                    temporary,
                    max_users,
                    pchat_mode: pchat_mode.map(|s| parse_pchat_mode_str(&s)),
                    pchat_max_history,
                    pchat_retention_days,
                })
                .await
                .map_err(|e| e.to_string())
            }
            None => Err("Not connected".into()),
        }
    }

    /// Delete a channel on the server.
    ///
    /// Sends `ChannelRemove` to the server.  The server will reject the
    /// request if the user lacks Write permission on the channel.  On
    /// success the server broadcasts `ChannelRemove` to all clients and
    /// (for `FancyMumble` servers) deletes all persistent-chat messages
    /// stored for that channel.
    pub async fn delete_channel(&self, channel_id: u32) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::DeleteChannel { channel_id })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Create a new sub-channel on the server.
    #[allow(clippy::too_many_arguments, reason = "channel creation mirrors the full server-side parameter surface as optional fields")]
    pub async fn create_channel(
        &self,
        parent_id: u32,
        name: String,
        description: Option<String>,
        position: Option<i32>,
        temporary: Option<bool>,
        max_users: Option<u32>,
        pchat_mode: Option<String>,
        pchat_max_history: Option<u32>,
        pchat_retention_days: Option<u32>,
    ) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => {
                h.send(command::SetChannelState {
                    channel_id: None,
                    parent: Some(parent_id),
                    name: Some(name),
                    description,
                    position,
                    temporary,
                    max_users,
                    pchat_mode: pchat_mode.map(|s| parse_pchat_mode_str(&s)),
                    pchat_max_history,
                    pchat_retention_days,
                })
                .await
                .map_err(|e| e.to_string())
            }
            None => Err("Not connected".into()),
        }
    }

    // -- Profile (comment / texture) ------------------------------

    /// Set the user's comment on the connected server.
    ///
    /// The comment is an `optional string` in protobuf - it must be valid
    /// UTF-8.  Binary payloads (e.g. banner images) are base64-encoded by
    /// the frontend before being embedded in the `FancyMumble` JSON marker.
    pub async fn set_user_comment(&self, comment: String) -> Result<(), String> {
        let (handle, own_session) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            (state.client_handle.clone(), state.own_session)
        };
        match handle {
            Some(h) => {
                h.send(command::SetComment {
                    comment: comment.clone(),
                })
                .await
                .map_err(|e| e.to_string())?;

                // Update local state immediately so the frontend doesn't
                // have to wait for the server echo (which may only carry
                // a comment_hash for large comments).
                if let Some(session) = own_session {
                    if let Ok(mut state) = self.inner.lock() {
                        if let Some(user) = state.users.get_mut(&session) {
                            user.comment = if comment.is_empty() {
                                None
                            } else {
                                Some(comment)
                            };
                        }
                    }
                    if let Some(app) = self.app_handle() {
                        let _ = app.emit("state-changed", ());
                    }
                }
                Ok(())
            }
            None => Err("Not connected".into()),
        }
    }

    /// Set the user's avatar texture on the connected server.
    ///
    /// `texture` is the raw image bytes (PNG / JPEG). Pass an empty
    /// `Vec` to clear the avatar.
    pub async fn set_user_texture(&self, texture: Vec<u8>) -> Result<(), String> {
        let (handle, own_session) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            (state.client_handle.clone(), state.own_session)
        };
        match handle {
            Some(h) => {
                h.send(command::SetTexture {
                    texture: texture.clone(),
                })
                .await
                .map_err(|e| e.to_string())?;

                // Update local state immediately so the profile card
                // reflects the new avatar without waiting for the server echo.
                if let Some(session) = own_session {
                    if let Ok(mut state) = self.inner.lock() {
                        if let Some(user) = state.users.get_mut(&session) {
                            user.texture = if texture.is_empty() {
                                None
                            } else {
                                Some(texture)
                            };
                        }
                    }
                    if let Some(app) = self.app_handle() {
                        let _ = app.emit("state-changed", ());
                    }
                }
                Ok(())
            }
            None => Err("Not connected".into()),
        }
    }

    // -- Admin actions --------------------------------------------

    /// Kick a user from the server.
    pub async fn kick_user(&self, session: u32, reason: Option<String>) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::KickUser { session, reason })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Ban a user from the server.
    pub async fn ban_user(&self, session: u32, reason: Option<String>) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::BanUser { session, reason })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Register a user on the server using their current certificate.
    pub async fn register_user(&self, session: u32) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RegisterUser { session })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Admin-mute or unmute another user.
    pub async fn mute_user(&self, session: u32, muted: bool) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::SetUserMute { session, muted })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Admin-deafen or undeafen another user.
    pub async fn deafen_user(&self, session: u32, deafened: bool) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::SetUserDeaf {
                    session,
                    deafened,
                })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Set or clear priority speaker for a user.
    pub async fn set_priority_speaker(
        &self,
        session: u32,
        priority: bool,
    ) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::SetPrioritySpeaker { session, priority })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Reset another user's comment (admin action).
    pub async fn reset_user_comment(&self, session: u32) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::ResetUserComment { session })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Remove another user's avatar (admin action).
    pub async fn remove_user_avatar(&self, session: u32) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RemoveUserAvatar { session })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Request statistics for a specific user from the server.
    ///
    /// The server replies with a `UserStats` message, which the handler
    /// emits as a `"user-stats"` event to the frontend.
    pub async fn request_user_stats(&self, session: u32) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RequestUserStats { session })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Request the registered user list from the server.
    ///
    /// The server replies with a `UserList` message, emitted as a
    /// `"user-list"` event to the frontend.
    pub async fn request_user_list(&self) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RequestUserList)
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Update registered users on the server (rename or delete).
    ///
    /// Each entry with `name: Some(new_name)` renames the user;
    /// entries with `name: None` deregister (delete) the user.
    pub async fn update_user_list(
        &self,
        users: Vec<RegisteredUserUpdate>,
    ) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        let entries = users
            .into_iter()
            .map(|u| command::UserListEntry {
                user_id: u.user_id,
                name: u.name,
            })
            .collect();
        match handle {
            Some(h) => h
                .send(command::UpdateUserList { users: entries })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Request the ban list from the server.
    ///
    /// The server replies with a `BanList` message, emitted as a
    /// `"ban-list"` event to the frontend.
    pub async fn request_ban_list(&self) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RequestBanList)
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Send an updated ban list to the server (replaces the entire list).
    pub async fn update_ban_list(
        &self,
        bans: Vec<BanEntryInput>,
    ) -> Result<(), String> {
        use mumble_protocol::proto::mumble_tcp;

        let entries: Result<Vec<_>, String> = bans
            .into_iter()
            .map(|b| {
                let address = fancy_utils::net::parse_ip_to_bytes(&b.address)?;
                Ok(mumble_tcp::ban_list::BanEntry {
                    address,
                    mask: b.mask,
                    name: if b.name.is_empty() { None } else { Some(b.name) },
                    hash: if b.hash.is_empty() { None } else { Some(b.hash) },
                    reason: if b.reason.is_empty() { None } else { Some(b.reason) },
                    start: if b.start.is_empty() { None } else { Some(b.start) },
                    duration: if b.duration == 0 { None } else { Some(b.duration) },
                })
            })
            .collect();
        let entries = entries?;

        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::SendBanList { bans: entries })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Request the ACL for a specific channel.
    ///
    /// The server replies with an `Acl` message, emitted as an
    /// `"acl"` event to the frontend.
    pub async fn request_acl(&self, channel_id: u32) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RequestAcl { channel_id })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Send an updated ACL for a channel to the server.
    pub async fn update_acl(&self, acl: AclInput) -> Result<(), String> {
        use mumble_protocol::proto::mumble_tcp;

        let groups: Vec<mumble_tcp::acl::ChanGroup> = acl
            .groups
            .into_iter()
            .map(|g| mumble_tcp::acl::ChanGroup {
                name: g.name,
                inherited: Some(g.inherited),
                inherit: Some(g.inherit),
                inheritable: Some(g.inheritable),
                add: g.add,
                remove: g.remove,
                inherited_members: g.inherited_members,
            })
            .collect();

        let acls: Vec<mumble_tcp::acl::ChanAcl> = acl
            .acls
            .into_iter()
            .map(|a| mumble_tcp::acl::ChanAcl {
                apply_here: Some(a.apply_here),
                apply_subs: Some(a.apply_subs),
                inherited: Some(a.inherited),
                user_id: a.user_id,
                group: a.group,
                grant: Some(a.grant),
                deny: Some(a.deny),
            })
            .collect();

        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::SendAcl {
                    channel_id: acl.channel_id,
                    inherit_acls: acl.inherit_acls,
                    groups,
                    acls,
                })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }
}


