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
pub mod types;

// Re-export everything that lib.rs needs.
pub use types::{
    AudioDevice, AudioSettings, ChannelEntry, ChatMessage, ConnectionStatus, ServerConfig,
    UserEntry, VoiceState,
};

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};
use tracing::info;

use mumble_protocol::audio::pipeline::InboundPipeline;
use mumble_protocol::client::ClientHandle;
use mumble_protocol::command;

use types::*;

// ─── Shared interior state ────────────────────────────────────────

#[derive(Default)]
pub(super) struct SharedState {
    pub status: ConnectionStatus,
    /// Monotonically increasing counter - bumped on every `connect()` call.
    /// Used to detect stale `on_disconnected` callbacks from orphaned tasks.
    pub connection_epoch: u64,
    pub client_handle: Option<ClientHandle>,
    /// `JoinHandle` for the event-loop task so we can await a clean shutdown.
    pub event_loop_handle: Option<tokio::task::JoinHandle<()>>,
    pub users: HashMap<u32, UserEntry>,
    pub channels: HashMap<u32, ChannelEntry>,
    /// `channel_id` → messages
    pub messages: HashMap<u32, Vec<ChatMessage>>,
    pub own_session: Option<u32>,
    pub own_name: String,
    /// Whether we've received `ServerSync` (initial state is complete).
    pub synced: bool,
    /// Channels the user has permanently opted to listen to (via context menu).
    pub permanently_listened: HashSet<u32>,
    /// The channel currently selected in the UI (viewing chat).
    pub selected_channel: Option<u32>,
    /// The channel the user is physically in (joined).
    pub current_channel: Option<u32>,
    /// Unread message counts per channel.
    pub unread_counts: HashMap<u32, u32>,
    /// Server-reported configuration limits.
    pub server_config: ServerConfig,
    /// Audio settings (device, gain, VAD threshold).
    pub audio_settings: AudioSettings,
    /// Whether voice calling is active (inactive = deaf+muted).
    pub voice_state: VoiceState,
    /// Inbound audio pipeline (network → speakers).
    pub inbound_pipeline: Option<InboundPipeline>,
    /// Handle to the outbound audio capture task (mic → network).
    pub outbound_task_handle: Option<tokio::task::JoinHandle<()>>,
}

// ─── Tauri-managed application state ──────────────────────────────

/// Central state managed by Tauri and shared across all commands.
pub struct AppState {
    pub(super) inner: Arc<Mutex<SharedState>>,
    app_handle: Mutex<Option<AppHandle>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SharedState::default())),
            app_handle: Mutex::new(None),
        }
    }

    /// Inject the Tauri `AppHandle` during setup.
    pub fn set_app_handle(&self, handle: AppHandle) {
        *self.app_handle.lock().unwrap() = Some(handle);
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

    // ── Query methods ─────────────────────────────────────────────

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
                let mut channels: Vec<_> = s.channels.values().cloned().collect();
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

    /// Return our own session ID (assigned by the server after connect).
    pub fn get_own_session(&self) -> Option<u32> {
        self.inner.lock().ok().and_then(|s| s.own_session)
    }

    // ── Messaging ─────────────────────────────────────────────────

    pub async fn send_message(&self, channel_id: u32, body: String) -> Result<(), String> {
        let (handle, own_session, own_name) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            (
                state.client_handle.clone(),
                state.own_session,
                state.own_name.clone(),
            )
        };

        let handle = handle.ok_or("Not connected")?;

        handle
            .send(command::SendTextMessage {
                channel_ids: vec![channel_id],
                user_sessions: vec![],
                tree_ids: vec![],
                message: body.clone(),
            })
            .await
            .map_err(|e| format!("Failed to send message: {e}"))?;

        // Add locally - the server does not echo our own messages back.
        if let Ok(mut state) = self.inner.lock() {
            state
                .messages
                .entry(channel_id)
                .or_default()
                .push(ChatMessage {
                    sender_session: own_session,
                    sender_name: own_name,
                    body,
                    channel_id,
                    is_own: true,
                });
        }

        Ok(())
    }

    // ── Plugin data ────────────────────────────────────────────────

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

    // ── Channel browse / join / listen ──────────────────────────────

    /// Select a channel in the UI for viewing (does NOT join it).
    pub fn select_channel(&self, channel_id: u32) -> Result<(), String> {
        {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.selected_channel = Some(channel_id);
            // Mark the channel as read.
            state.unread_counts.remove(&channel_id);
        }
        self.emit_unreads();
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
    pub async fn toggle_listen(&self, channel_id: u32) -> Result<bool, String> {
        info!(channel_id, "toggle_listen called");
        let (handle, is_now_listened, add, remove) = {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            let handle = state.client_handle.clone();

            if state.permanently_listened.contains(&channel_id) {
                // Unlisten - but keep listening if it's the selected channel.
                state.permanently_listened.remove(&channel_id);
                let is_selected = state.selected_channel == Some(channel_id);
                if is_selected {
                    // Still auto-listened, don't remove from server.
                    (handle, false, vec![], vec![])
                } else {
                    (handle, false, vec![], vec![channel_id])
                }
            } else {
                // Start permanent listen.
                state.permanently_listened.insert(channel_id);
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
            state.unread_counts.remove(&channel_id);
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

    /// Get the server-reported config limits.
    pub fn server_config(&self) -> ServerConfig {
        self.inner
            .lock()
            .map(|s| s.server_config.clone())
            .unwrap_or_default()
    }

    // ── Profile (comment / texture) ──────────────────────────────

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
}
