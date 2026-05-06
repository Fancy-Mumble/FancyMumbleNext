//! Shared application state for the Tauri backend.
//!
//! The state is decomposed into sub-modules by domain:
//!
//! - [`types`]              - serializable value types, event payloads, config structs.
//! - [`connection`]         - `connect()` / `disconnect()` lifecycle.
//! - [`audio`]              - voice pipeline management (enable, mute, deafen, outbound loop).
//! - [`event_handler`]      - `EventHandler` bridge from mumble-protocol to Tauri events.
//! - [`messaging`]          - channel / DM / group messaging, unread tracking.
//! - [`channels`]           - channel browse, join, listen, create, update, delete.
//! - [`admin`]              - server administration actions.
//! - [`profile`]            - user comment and avatar management.
//! - [`protocol_commands`]  - protocol-level commands (plugin data, reactions, etc.).
//! - [`query`]              - read-only accessors (status, users, server info, etc.).
//! - [`offload_ops`]        - content offloading to encrypted temp files.

mod admin;
mod audio;
mod audio_tasks;
mod channels;
mod connection;
mod emotes;
pub use emotes::{AddEmoteRequest, AddEmoteResponse, RemoveEmoteRequest};
mod event_handler;
mod file_server;
pub use file_server::{DownloadRequest, UploadRequest, UploadResponse};
mod handler;
pub(crate) mod hash_names;
pub(crate) mod local_cache;
mod messaging;
pub mod offload;
mod offload_ops;
pub(crate) mod pchat;
mod profile;
pub(crate) mod protocol_commands;
mod query;
#[allow(dead_code, reason = "recording module is work-in-progress")]
pub(crate) mod recording;
mod registry;
mod search;
mod sessions;
mod shared_handle;
pub mod types;

// Re-export everything that lib.rs needs.
pub use sessions::{ServerId, SessionMeta};
pub use types::{
    AudioDevice, AudioSettings, ChannelEntry, ChatMessage, ConnectionStatus, DebugStats,
    PhotoEntry, SearchResult, ServerConfig, ServerInfo, UserEntry, VoiceState,
};

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

use offload::OffloadStore;

use mumble_protocol::audio::mixer::{AudioMixer, SpeakerVolumes};
use mumble_protocol::client::ClientHandle;
use mumble_protocol::persistent::PchatProtocol;

use types::*;

/// Parse a frontend pchat mode string into the protobuf i32 value.
pub(crate) fn parse_pchat_protocol_str(s: &str) -> PchatProtocol {
    match s {
        "fancy_v1_full_archive" => PchatProtocol::FancyV1FullArchive,
        "signal_v1" => PchatProtocol::SignalV1,
        _ => PchatProtocol::None,
    }
}

// --- Sub-state structs --------------------------------------------

/// Audio pipeline state: device settings, volume handles, mixer,
/// playback, outbound capture, and test tasks.
#[derive(Default)]
pub(super) struct AudioPipelineState {
    pub settings: AudioSettings,
    pub voice_state: VoiceState,
    pub mixer: Option<AudioMixer>,
    pub mixing_playback: Option<Box<dyn crate::audio::MixingPlayback>>,
    pub outbound_task_handle: Option<tokio::task::JoinHandle<()>>,
    pub input_volume_handle: Option<Arc<AtomicU32>>,
    pub output_volume_handle: Option<Arc<AtomicU32>>,
    pub speaker_volumes: SpeakerVolumes,
    pub mic_test_handle: Option<tauri::async_runtime::JoinHandle<()>>,
    pub latency_test_handle: Option<tauri::async_runtime::JoinHandle<()>>,
    pub recording_handle: Option<recording::RecordingHandle>,
    pub talking_sessions: HashSet<u32>,
}

/// Server-reported metadata: version info, config limits, and connection details.
#[derive(Default)]
pub(super) struct ServerMetadata {
    pub fancy_version: Option<u64>,
    pub version_info: ServerVersionInfo,
    pub host: String,
    pub port: u16,
    pub max_users: Option<u32>,
    pub max_bandwidth: Option<u32>,
    pub opus: bool,
    pub config: ServerConfig,
    pub welcome_text: Option<String>,
    pub root_permissions: Option<u32>,
}

/// User-level preference flags.
#[derive(Default, Clone)]
pub(super) struct AppPreferences {
    pub notifications_enabled: bool,
    pub disable_dual_path: bool,
    pub app_focused: bool,
}

/// Persistent-chat context: key management, identity, and pending operations.
#[derive(Default)]
pub(super) struct PchatContext {
    pub pchat: Option<pchat::PchatState>,
    pub seed: Option<[u8; 32]>,
    pub identity_dir: Option<std::path::PathBuf>,
    pub pending_key_shares: Vec<PendingKeyShare>,
    pub key_holders: HashMap<u32, Vec<KeyHolderEntry>>,
    pub hash_name_resolver: Option<Arc<dyn hash_names::HashNameResolver>>,
    pub pending_delete_acks: Vec<tokio::sync::oneshot::Sender<DeleteAckResult>>,
}

/// Maximum number of in-memory messages retained per thread (channel or DM).
/// Older messages remain available through the persistent local cache and
/// can be loaded on demand via `fetch_older_messages`.  Capping the working
/// set keeps long-running sessions from accumulating unbounded memory and
/// prevents the UI from re-rendering ever-growing lists.
pub(super) const MAX_MESSAGES_PER_THREAD: usize = 500;

/// Append a message to a thread's `Vec<ChatMessage>` while enforcing the
/// `MAX_MESSAGES_PER_THREAD` cap by dropping the oldest entries.
pub(super) fn push_capped(messages: &mut Vec<ChatMessage>, msg: ChatMessage) {
    messages.push(msg);
    if messages.len() > MAX_MESSAGES_PER_THREAD {
        let drop_count = messages.len() - MAX_MESSAGES_PER_THREAD;
        let _ = messages.drain(..drop_count);
    }
}

/// Message storage: channel and DM messages with unread counts.
#[derive(Default)]
pub(super) struct MessageStore {
    pub by_channel: HashMap<u32, Vec<ChatMessage>>,
    pub by_dm: HashMap<u32, Vec<ChatMessage>>,
    pub channel_unread: HashMap<u32, u32>,
    pub dm_unread: HashMap<u32, u32>,
    pub selected_dm_user: Option<u32>,
}

/// Connection lifecycle state.
#[derive(Default)]
pub(super) struct ConnectionFields {
    pub status: ConnectionStatus,
    pub epoch: u64,
    pub client_handle: Option<ClientHandle>,
    pub connect_task_handle: Option<tokio::task::JoinHandle<()>>,
    pub event_loop_handle: Option<tokio::task::JoinHandle<()>>,
    pub synced: bool,
    pub own_session: Option<u32>,
    pub own_name: String,
    pub user_initiated_disconnect: bool,
    pub tauri_app_handle: Option<AppHandle>,
}

// --- Shared interior state (composed) -----------------------------

#[derive(Default)]
pub(super) struct SharedState {
    pub conn: ConnectionFields,
    pub server: ServerMetadata,
    pub users: HashMap<u32, UserEntry>,
    pub channels: HashMap<u32, ChannelEntry>,
    pub selected_channel: Option<u32>,
    pub current_channel: Option<u32>,
    pub permanently_listened: HashSet<u32>,
    pub push_subscribed_channels: HashSet<u32>,
    pub msgs: MessageStore,
    pub audio: AudioPipelineState,
    pub pchat_ctx: PchatContext,
    pub prefs: AppPreferences,
    pub offload_store: Option<OffloadStore>,
    /// Multi-server: stable id of the connection this state belongs to.
    /// Set when the session is registered, cleared on teardown.
    pub server_id: Option<ServerId>,
    /// Certificate label used for this connection (if any), kept so
    /// `list_servers` can surface it without re-querying the connect args.
    pub cert_label: Option<String>,
}

// --- Tauri-managed application state ------------------------------

/// Central state managed by Tauri and shared across all commands.
pub struct AppState {
    /// Atomically-swappable handle to the currently-active session's
    /// [`SharedState`].  Existing call sites continue to spell
    /// `state.inner.snapshot().lock()`; under the hood the lock targets whichever
    /// session is active at lock time.
    pub(crate) inner: shared_handle::SharedHandle,
    /// Multi-server registry mapping `ServerId -> Arc<Mutex<SharedState>>`,
    /// plus which session is currently active.  Each connected server
    /// has its own backing `SharedState` so per-server data (channels,
    /// users, messages, audio) stays isolated.
    pub(crate) registry: registry::Registry,
    /// Default empty `SharedState` selected when no server is connected,
    /// so commands can always lock something and observe a sensible
    /// disconnected view instead of failing.
    default_inner: Arc<Mutex<SharedState>>,
    app_handle: Mutex<Option<AppHandle>>,
    start_time: Instant,
    http_client: reqwest::Client,
    pub(super) upload_cancels: Mutex<HashMap<String, CancellationToken>>,
    /// Image sources pending pickup by freshly-opened image popout windows.
    /// Keyed by random id; each entry is consumed once by `take_popout_image`.
    pub(crate) popout_images: Mutex<HashMap<String, crate::commands::popout::PopoutImagePayload>>,
    /// Channel/session context for the (single) drawing-overlay window.
    /// Read by the overlay via `take_drawing_overlay_context` once it
    /// has spawned. `None` while no overlay is open.
    pub(crate) draw_overlay_context:
        Mutex<Option<crate::commands::draw_overlay::DrawOverlayContext>>,
    /// Background task that follows the shared window's screen rect
    /// (Windows only) and repositions the desktop overlay accordingly.
    /// Aborted when the overlay closes.
    pub(crate) draw_overlay_tracker: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl AppState {
    pub fn new() -> Self {
        let default_inner = Arc::new(Mutex::new(SharedState {
            prefs: AppPreferences {
                notifications_enabled: true,
                app_focused: true,
                ..Default::default()
            },
            ..Default::default()
        }));
        Self {
            registry: registry::Registry::default(),
            inner: shared_handle::SharedHandle::new(Arc::clone(&default_inner)),
            default_inner,
            app_handle: Mutex::new(None),
            start_time: Instant::now(),
            http_client: file_server::new_http_client(),
            upload_cancels: Mutex::new(HashMap::new()),
            popout_images: Mutex::new(HashMap::new()),
            draw_overlay_context: Mutex::new(None),
            draw_overlay_tracker: Mutex::new(None),
        }
    }

    /// Build a fresh, empty per-session `SharedState` seeded with the
    /// global preferences and audio settings from the default session.
    /// Audio settings are copied so that persisted preferences (device,
    /// bitrate, denoiser, VAD threshold, …) apply to the very first
    /// voice call without requiring the settings page to be opened.
    pub(crate) fn fresh_session_state(&self) -> Arc<Mutex<SharedState>> {
        let (prefs, audio_settings) = self
            .default_inner
            .lock()
            .map(|s| (s.prefs.clone(), s.audio.settings.clone()))
            .unwrap_or_default();
        Arc::new(Mutex::new(SharedState {
            prefs,
            audio: AudioPipelineState {
                settings: audio_settings,
                ..Default::default()
            },
            ..Default::default()
        }))
    }

    /// Switch the currently-active session to `target`.  Updates both
    /// the registry's `active` pointer and the `inner` swap so commands
    /// without an explicit `serverId` start operating on the new session.
    pub(crate) fn switch_active_to(&self, target: ServerId) -> Result<(), String> {
        let arc = self
            .registry
            .session(target)
            .ok_or_else(|| format!("unknown server id: {target}"))?;
        self.registry.set_active(target)?;
        let _ = self.inner.swap(arc);
        Ok(())
    }

    /// Switch the active session to `target` and migrate any running
    /// voice pipeline along with it.  If voice was Active or Muted on
    /// the previously-active session, it is stopped there and started
    /// on the new active session in the same mode.  Voice always
    /// follows the active server.
    pub(crate) async fn switch_active_with_voice(
        &self,
        target: ServerId,
    ) -> Result<(), String> {
        use crate::state::types::VoiceState;

        let prev_arc = self.inner.snapshot();
        let target_arc = self
            .registry
            .session(target)
            .ok_or_else(|| format!("unknown server id: {target}"))?;

        if Arc::ptr_eq(&prev_arc, &target_arc) {
            return Ok(());
        }
        drop(target_arc);

        let prev_voice = prev_arc
            .lock()
            .map(|s| s.audio.voice_state)
            .unwrap_or_default();

        if prev_voice != VoiceState::Inactive {
            self.stop_audio_on(&prev_arc);
            if let Ok(mut s) = prev_arc.lock() {
                s.audio.voice_state = VoiceState::Inactive;
            }
        }

        self.switch_active_to(target)?;

        match prev_voice {
            VoiceState::Inactive => {}
            VoiceState::Active => {
                if let Err(e) = self.enable_voice().await {
                    tracing::warn!("voice migration: enable_voice on new active failed: {e}");
                }
            }
            VoiceState::Muted => {
                if let Err(e) = self.enable_voice_muted().await {
                    tracing::warn!(
                        "voice migration: enable_voice_muted on new active failed: {e}"
                    );
                }
            }
        }

        self.emit_voice_state();
        Ok(())
    }

    /// Reset `inner` to the empty default `SharedState` (used when the
    /// last session disconnects).
    pub(crate) fn reset_to_default(&self) {
        let _ = self.inner.swap(Arc::clone(&self.default_inner));
    }

    /// Inject the Tauri `AppHandle` during setup.
    pub fn set_app_handle(&self, handle: AppHandle) {
        *self.app_handle.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
    }

    pub(super) fn app_handle(&self) -> Option<AppHandle> {
        self.app_handle.lock().ok().and_then(|h| h.clone())
    }

    /// Cancel an in-progress upload by its `upload_id`.
    /// Returns `true` if a matching upload was found and cancelled.
    pub fn cancel_upload(&self, upload_id: &str) -> bool {
        if let Ok(mut map) = self.upload_cancels.lock() {
            if let Some(token) = map.remove(upload_id) {
                token.cancel();
                return true;
            }
        }
        false
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
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "panic-on-failure is acceptable in test code"
)]
mod tests {
    use super::*;

    fn dummy_message(idx: usize) -> ChatMessage {
        ChatMessage {
            sender_session: Some(1),
            sender_name: "user".into(),
            sender_hash: None,
            body: format!("msg {idx}"),
            channel_id: 0,
            is_own: false,
            dm_session: None,
            message_id: Some(format!("id-{idx}")),
            timestamp: Some(idx as u64),
            is_legacy: false,
            edited_at: None,
            pinned: false,
            pinned_by: None,
            pinned_at: None,
        }
    }

    #[test]
    fn push_capped_drops_oldest_when_full() {
        let mut buf: Vec<ChatMessage> = Vec::new();
        for i in 0..(MAX_MESSAGES_PER_THREAD + 5) {
            push_capped(&mut buf, dummy_message(i));
        }
        assert_eq!(buf.len(), MAX_MESSAGES_PER_THREAD);
        // Oldest 5 should have been drained; first remaining message is index 5.
        assert_eq!(buf.first().and_then(|m| m.timestamp), Some(5));
        assert_eq!(
            buf.last().and_then(|m| m.timestamp),
            Some((MAX_MESSAGES_PER_THREAD + 4) as u64),
        );
    }

    #[test]
    fn push_capped_below_limit_keeps_all() {
        let mut buf: Vec<ChatMessage> = Vec::new();
        for i in 0..10 {
            push_capped(&mut buf, dummy_message(i));
        }
        assert_eq!(buf.len(), 10);
        assert_eq!(buf.first().and_then(|m| m.timestamp), Some(0));
    }

    // Phase E: voice migration on active-session switch.
    //
    // We cover the safe branches that don't try to start a real audio
    // pipeline: unknown target, same-as-active no-op, and the
    // VoiceState::Inactive case where migration just performs the swap.

    fn register_idle_session(state: &AppState) -> ServerId {
        let id = ServerId::new();
        let arc = state.fresh_session_state();
        let _ = state.registry.register_active(id, arc);
        id
    }


    #[tokio::test]
    async fn switch_active_with_voice_unknown_target_errors() {
        let state = AppState::new();
        let _ = register_idle_session(&state);
        let bogus = ServerId::new();
        assert!(state.switch_active_with_voice(bogus).await.is_err());
    }

    #[tokio::test]
    async fn switch_active_with_voice_same_session_is_noop() {
        let state = AppState::new();
        let id = register_idle_session(&state);
        // Point `inner` at the registered session (registration alone
        // only updates the registry).
        state.switch_active_to(id).expect("ok");
        let prev = state.inner.snapshot();
        state.switch_active_with_voice(id).await.expect("ok");
        let next = state.inner.snapshot();
        assert!(Arc::ptr_eq(&prev, &next));
    }

    #[tokio::test]
    async fn switch_active_with_voice_inactive_just_swaps() {
        let state = AppState::new();
        let a = register_idle_session(&state);
        let b = register_idle_session(&state);
        // Most recently registered wins active in the registry.
        assert_eq!(state.registry.active_id(), Some(b));
        state.switch_active_to(b).expect("ok");
        state.switch_active_with_voice(a).await.expect("ok");
        assert_eq!(state.registry.active_id(), Some(a));
        let active_arc = state.registry.session(a).expect("a present");
        assert!(Arc::ptr_eq(&state.inner.snapshot(), &active_arc));
    }

    // Regression: disconnecting a non-active session must NOT touch
    // the active session's `inner` pointer, the active session's
    // SharedState, or the active session's audio pipeline.

    #[tokio::test]
    async fn disconnect_session_non_active_leaves_active_inner_intact() {
        let state = AppState::new();
        let active = register_idle_session(&state);
        let victim = register_idle_session(&state);
        state.switch_active_to(active).expect("ok");

        let active_arc_before = state.registry.session(active).expect("active");
        assert!(Arc::ptr_eq(&state.inner.snapshot(), &active_arc_before));

        state.disconnect_session(victim).await.expect("ok");

        // Victim removed from registry.
        assert!(state.registry.session(victim).is_none());
        // Active still active and `inner` still points at it.
        assert_eq!(state.registry.active_id(), Some(active));
        assert!(Arc::ptr_eq(&state.inner.snapshot(), &active_arc_before));
    }

    #[tokio::test]
    async fn disconnect_session_active_rebinds_inner_to_remaining() {
        let state = AppState::new();
        let other = register_idle_session(&state);
        let active = register_idle_session(&state);
        state.switch_active_to(active).expect("ok");

        state.disconnect_session(active).await.expect("ok");

        // Active gone, the remaining session takes over.
        assert!(state.registry.session(active).is_none());
        assert_eq!(state.registry.active_id(), Some(other));
        let other_arc = state.registry.session(other).expect("other present");
        assert!(Arc::ptr_eq(&state.inner.snapshot(), &other_arc));
    }

    #[tokio::test]
    async fn disconnect_session_last_resets_to_default() {
        let state = AppState::new();
        let only = register_idle_session(&state);
        state.switch_active_to(only).expect("ok");

        state.disconnect_session(only).await.expect("ok");

        assert!(state.registry.active_id().is_none());
        // `inner` must point at the empty default state.
        assert!(Arc::ptr_eq(&state.inner.snapshot(), &state.default_inner));
    }

    #[tokio::test]
    async fn disconnect_session_unknown_id_errors() {
        let state = AppState::new();
        let _active = register_idle_session(&state);
        let bogus = ServerId::new();
        assert!(state.disconnect_session(bogus).await.is_err());
    }
}
