//! `EventHandler` implementation that bridges mumble-protocol events
//! to the React frontend via Tauri's event system.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use tracing::{info, warn};

use mumble_protocol::audio::encoder::EncodedPacket;
use mumble_protocol::command;
use mumble_protocol::event::EventHandler;
use mumble_protocol::message::{ControlMessage, UdpMessage};

use super::types::*;
use super::SharedState;

/// Implements `EventHandler` to receive protocol events and push them
/// to the React frontend via Tauri's event system.
pub(super) struct TauriEventHandler {
    pub shared: Arc<Mutex<SharedState>>,
    pub app: AppHandle,
    /// Snapshot of `SharedState::connection_epoch` at construction time.
    /// `on_disconnected` only acts when this matches the current epoch,
    /// preventing stale callbacks from orphaned tasks.
    pub epoch: u64,
}

impl EventHandler for TauriEventHandler {
    fn on_control_message(&mut self, msg: &ControlMessage) {
        match msg {
            ControlMessage::ServerSync(sync) => {
                let sessions: Vec<u32>;
                let initial_channel: Option<u32>;
                {
                    let mut state = match self.shared.lock() {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    state.status = ConnectionStatus::Connected;
                    state.own_session = sync.session;
                    state.synced = true;

                    // Now that we know our session, look up the channel
                    // from UserState messages that arrived before ServerSync.
                    initial_channel = sync
                        .session
                        .and_then(|s| state.users.get(&s))
                        .map(|u| u.channel_id);
                    if let Some(ch) = initial_channel {
                        state.current_channel = Some(ch);
                    }

                    // Collect all user sessions so we can request their
                    // texture + comment blobs from the server.
                    sessions = state.users.keys().copied().collect();
                }
                let _ = self.app.emit("server-connected", ());

                // Notify frontend about the initial channel assignment.
                if let Some(ch) = initial_channel {
                    let _ = self.app.emit(
                        "current-channel-changed",
                        CurrentChannelPayload { channel_id: ch },
                    );
                }

                // Request full texture & comment blobs for every user.
                if !sessions.is_empty() {
                    let shared = Arc::clone(&self.shared);
                    tokio::spawn(async move {
                        let handle = {
                            let state = shared.lock().ok();
                            state.and_then(|s| s.client_handle.clone())
                        };
                        if let Some(handle) = handle {
                            let _ = handle
                                .send(command::RequestBlob {
                                    session_texture: sessions.clone(),
                                    session_comment: sessions,
                                })
                                .await;
                        }
                    });
                }
            }

            ControlMessage::UserState(us) => {
                if let Some(session) = us.session {
                    let (is_synced, own_channel_changed) = {
                        let mut state_guard = self.shared.lock().ok();
                        if let Some(ref mut state) = state_guard {
                            let user =
                                state.users.entry(session).or_insert_with(|| UserEntry {
                                    session,
                                    name: String::new(),
                                    channel_id: 0,
                                    texture: None,
                                    comment: None,
                                });
                            if let Some(ref name) = us.name {
                                user.name = name.clone();
                            }
                            if let Some(ref texture) = us.texture {
                                user.texture = if texture.is_empty() {
                                    None
                                } else {
                                    Some(texture.clone())
                                };
                            }
                            if let Some(ref comment) = us.comment {
                                user.comment = if comment.is_empty() {
                                    None
                                } else {
                                    Some(comment.clone())
                                };
                            }
                            let mut own_ch = false;
                            if let Some(ch) = us.channel_id {
                                user.channel_id = ch;
                                // Track when our own user moves channels.
                                if state.own_session == Some(session) {
                                    state.current_channel = Some(ch);
                                    own_ch = true;
                                }
                            }
                            (state.synced, own_ch)
                        } else {
                            (false, false)
                        }
                    };
                    // Notify frontend about current-channel change.
                    if own_channel_changed {
                        if let Some(ch) = us.channel_id {
                            let _ = self.app.emit(
                                "current-channel-changed",
                                CurrentChannelPayload { channel_id: ch },
                            );
                        }
                    }
                    // Only notify frontend after initial sync is done.
                    if is_synced {
                        let _ = self.app.emit("state-changed", ());
                    }
                }
            }

            ControlMessage::UserRemove(ur) => {
                let is_self_kicked = {
                    let state = self.shared.lock().ok();
                    state.and_then(|s| s.own_session) == Some(ur.session)
                };

                if is_self_kicked {
                    // We got kicked/banned - clean up and notify frontend.
                    let reason = ur
                        .reason
                        .clone()
                        .unwrap_or_else(|| "Disconnected by server".into());
                    info!("Kicked from server: {reason}");
                    if let Ok(mut state) = self.shared.lock() {
                        state.status = ConnectionStatus::Disconnected;
                        state.client_handle = None;
                        state.event_loop_handle = None;
                        state.users.clear();
                        state.channels.clear();
                        state.messages.clear();
                        state.own_session = None;
                        state.synced = false;
                        state.permanently_listened.clear();
                        state.selected_channel = None;
                        state.current_channel = None;
                        state.unread_counts.clear();
                        state.server_config = ServerConfig::default();
                        state.voice_state = VoiceState::Inactive;
                        // Stop audio pipelines.
                        if let Some(handle) = state.outbound_task_handle.take() {
                            handle.abort();
                        }
                        state.inbound_pipeline = None;
                    }
                    let _ = self.app.emit(
                        "connection-rejected",
                        RejectedPayload { reason },
                    );
                    let _ = self.app.emit("server-disconnected", ());
                } else {
                    if let Ok(mut state) = self.shared.lock() {
                        state.users.remove(&ur.session);
                    }
                    let _ = self.app.emit("state-changed", ());
                }
            }

            ControlMessage::ChannelState(cs) => {
                if let Some(id) = cs.channel_id {
                    let is_synced = {
                        let mut state_guard = self.shared.lock().ok();
                        if let Some(ref mut state) = state_guard {
                            let ch =
                                state.channels.entry(id).or_insert_with(|| ChannelEntry {
                                    id,
                                    parent_id: None,
                                    name: String::new(),
                                    description: String::new(),
                                    user_count: 0,
                                });
                            if let Some(parent) = cs.parent {
                                ch.parent_id = Some(parent);
                            }
                            if let Some(ref name) = cs.name {
                                ch.name = name.clone();
                            }
                            if let Some(ref desc) = cs.description {
                                ch.description = desc.clone();
                            }
                            state.synced
                        } else {
                            false
                        }
                    };
                    // Only notify frontend after initial sync is done.
                    if is_synced {
                        let _ = self.app.emit("state-changed", ());
                    }
                }
            }

            ControlMessage::ChannelRemove(cr) => {
                if let Ok(mut state) = self.shared.lock() {
                    state.channels.remove(&cr.channel_id);
                    state.messages.remove(&cr.channel_id);
                }
                let _ = self.app.emit("state-changed", ());
            }

            ControlMessage::TextMessage(tm) => {
                let mut unreads_changed = false;
                if let Ok(mut state) = self.shared.lock() {
                    let actor = tm.actor;
                    let own_session = state.own_session;

                    // Don't duplicate messages we sent ourselves.
                    let is_own = actor == own_session && actor.is_some();
                    if is_own {
                        return;
                    }

                    let sender_name = actor
                        .and_then(|sid| state.users.get(&sid))
                        .map(|u| u.name.clone())
                        .unwrap_or_else(|| "Server".into());

                    let body = tm.message.clone();

                    let target_channels: Vec<u32> = if tm.channel_id.is_empty() {
                        vec![0]
                    } else {
                        tm.channel_id.clone()
                    };

                    let selected = state.selected_channel;

                    for &ch_id in &target_channels {
                        state
                            .messages
                            .entry(ch_id)
                            .or_default()
                            .push(ChatMessage {
                                sender_session: actor,
                                sender_name: sender_name.clone(),
                                body: body.clone(),
                                channel_id: ch_id,
                                is_own: false,
                            });

                        // Increment unread count if this channel is not currently viewed.
                        if selected != Some(ch_id) {
                            *state.unread_counts.entry(ch_id).or_insert(0) += 1;
                            unreads_changed = true;
                        }

                        let _ = self
                            .app
                            .emit("new-message", NewMessagePayload { channel_id: ch_id });

                        // Flash the taskbar on Windows when a permanently-listened
                        // channel gets a message while it is not the viewed channel.
                        if state.permanently_listened.contains(&ch_id)
                            && selected != Some(ch_id)
                        {
                            #[cfg(target_os = "windows")]
                            if let Some(window) = self.app.get_webview_window("main") {
                                let _ = window.request_user_attention(Some(
                                    tauri::UserAttentionType::Informational,
                                ));
                            }
                        }
                    }
                }
                if unreads_changed {
                    let unreads = self
                        .shared
                        .lock()
                        .map(|s| s.unread_counts.clone())
                        .unwrap_or_default();
                    let _ = self
                        .app
                        .emit("unread-changed", UnreadPayload { unreads });
                }
            }

            ControlMessage::Reject(r) => {
                let reason = r
                    .reason
                    .clone()
                    .unwrap_or_else(|| "Connection rejected by server".into());
                if let Ok(mut state) = self.shared.lock() {
                    state.status = ConnectionStatus::Disconnected;
                    state.client_handle = None;
                    state.event_loop_handle = None;
                }
                let _ = self
                    .app
                    .emit("connection-rejected", RejectedPayload { reason });
            }

            ControlMessage::ServerConfig(sc) => {
                if let Ok(mut state) = self.shared.lock() {
                    if let Some(len) = sc.message_length {
                        state.server_config.max_message_length = len;
                    }
                    if let Some(len) = sc.image_message_length {
                        // 0 means "no special limit" in the Mumble protocol;
                        // keep the default (131072) rather than storing 0.
                        if len > 0 {
                            state.server_config.max_image_message_length = len;
                        }
                    }
                    if let Some(allow) = sc.allow_html {
                        state.server_config.allow_html = allow;
                    }
                    info!(
                        msg_len = state.server_config.max_message_length,
                        img_len = state.server_config.max_image_message_length,
                        allow_html = state.server_config.allow_html,
                        "server config received"
                    );
                }
                let _ = self.app.emit("server-config", ());
            }

            ControlMessage::PermissionDenied(pd) => {
                // type 1 = Permission.  If the user tried to listen to a
                // channel they don't have permission for, revert the
                // permanently_listened set and notify the frontend.
                info!(reason = ?pd.reason, r#type = ?pd.r#type, channel_id = ?pd.channel_id,
                      "permission denied received");

                if let Some(ch_id) = pd.channel_id {
                    if let Ok(mut state) = self.shared.lock() {
                        if state.permanently_listened.remove(&ch_id) {
                            info!(ch_id, "reverted permanent listen due to permission denied");
                        }
                    }
                    let _ = self.app.emit(
                        "listen-denied",
                        ListenDeniedPayload { channel_id: ch_id },
                    );
                }

                // Always emit a general permission-denied event so the
                // frontend can surface errors (e.g. profile too large).
                let _ = self.app.emit(
                    "permission-denied",
                    PermissionDeniedPayload {
                        deny_type: pd.r#type,
                        reason: pd.reason.clone(),
                    },
                );
            }

            ControlMessage::PluginDataTransmission(pd) => {
                info!(
                    sender = ?pd.sender_session,
                    data_id = ?pd.data_id,
                    data_len = pd.data.as_ref().map(|d| d.len()).unwrap_or(0),
                    "plugin data received"
                );
                let _ = self.app.emit(
                    "plugin-data",
                    PluginDataPayload {
                        sender_session: pd.sender_session,
                        data: pd.data.clone().unwrap_or_default(),
                        data_id: pd.data_id.clone().unwrap_or_default(),
                    },
                );
            }

            _ => {}
        }
    }

    fn on_connected(&mut self) {
        info!("protocol: connected (ServerSync received)");
    }

    fn on_udp_message(&mut self, msg: &UdpMessage) {
        if let UdpMessage::Audio(audio) = msg {
            if audio.opus_data.is_empty() {
                return;
            }
            let packet = EncodedPacket {
                data: audio.opus_data.clone(),
                sequence: audio.frame_number,
                frame_samples: 960, // 20 ms @ 48 kHz (Opus reports actual size)
            };
            if let Ok(mut state) = self.shared.lock() {
                if let Some(ref mut pipeline) = state.inbound_pipeline {
                    if let Err(e) = pipeline.tick(&packet) {
                        warn!("inbound audio decode error: {e}");
                    }
                }
            }
        }
    }

    fn on_disconnected(&mut self) {
        if let Ok(mut state) = self.shared.lock() {
            // If the epoch has moved on, a newer `connect()` call has already
            // claimed the shared state.  Silently bail - this callback comes
            // from an orphaned / aborted event loop.
            if state.connection_epoch != self.epoch {
                info!(
                    handler_epoch = self.epoch,
                    current_epoch = state.connection_epoch,
                    "stale on_disconnected ignored"
                );
                return;
            }

            state.status = ConnectionStatus::Disconnected;
            state.client_handle = None;
            state.event_loop_handle = None;
            // Stop audio pipelines on disconnect.
            if let Some(handle) = state.outbound_task_handle.take() {
                handle.abort();
            }
            state.inbound_pipeline = None;
            state.voice_state = VoiceState::Inactive;
        }
        let _ = self.app.emit("server-disconnected", ());
    }
}
