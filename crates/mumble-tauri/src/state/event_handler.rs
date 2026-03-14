//! `EventHandler` implementation that bridges mumble-protocol events
//! to the React frontend via Tauri's event system.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};
#[cfg(target_os = "windows")]
use tauri::Manager;
use tracing::info;
#[cfg(not(target_os = "android"))]
use tracing::warn;

#[cfg(not(target_os = "android"))]
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
            ControlMessage::Ping(ping) => {
                if let Some(ts) = ping.timestamp {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let rtt_ms = now.saturating_sub(ts) as f64;
                    let _ = self
                        .app
                        .emit("ping-latency", LatencyPayload { rtt_ms });
                }
            }

            ControlMessage::Version(v) => {
                if let Ok(mut state) = self.shared.lock() {
                    state.server_fancy_version = v.fancy_version;
                    state.server_version_info = ServerVersionInfo {
                        release: v.release.clone(),
                        os: v.os.clone(),
                        os_version: v.os_version.clone(),
                        version_v1: v.version_v1,
                        version_v2: v.version_v2,
                        fancy_version: v.fancy_version,
                    };
                }
            }

            ControlMessage::ServerSync(sync) => {
                let sessions: Vec<u32>;
                let initial_channel: Option<u32>;
                {
                    let Ok(mut state) = self.shared.lock() else { return };
                    state.status = ConnectionStatus::Connected;
                    state.own_session = sync.session;
                    state.synced = true;
                    state.max_bandwidth = sync.max_bandwidth;
                    state.welcome_text = sync.welcome_text.clone();

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

                // Request permissions for all known channels so the UI
                // can grey out actions the user is not allowed to perform.
                {
                    let channel_ids: Vec<u32>;
                    {
                        let state = self.shared.lock().ok();
                        channel_ids = state
                            .map(|s| s.channels.keys().copied().collect())
                            .unwrap_or_default();
                    }
                    let shared = Arc::clone(&self.shared);
                    tokio::spawn(async move {
                        let handle = {
                            let state = shared.lock().ok();
                            state.and_then(|s| s.client_handle.clone())
                        };
                        if let Some(handle) = handle {
                            for ch_id in channel_ids {
                                let _ = handle
                                    .send(command::PermissionQuery { channel_id: ch_id })
                                    .await;
                            }
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
                        state.server_fancy_version = None;
                        state.server_version_info = ServerVersionInfo::default();
                        state.max_users = None;
                        state.max_bandwidth = None;
                        state.opus = false;
                        // Stop audio pipelines (desktop only).
                        #[cfg(not(target_os = "android"))]
                        {
                            if let Some(handle) = state.outbound_task_handle.take() {
                                handle.abort();
                            }
                            state.inbound_pipeline = None;
                        }
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
                                    permissions: None,
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

                    // When a channel state changes, re-query its permissions
                    // so the cached bitmask stays up-to-date (ACL changes, etc.).
                    if is_synced {
                        let shared = Arc::clone(&self.shared);
                        tokio::spawn(async move {
                            let handle = {
                                let state = shared.lock().ok();
                                state.and_then(|s| s.client_handle.clone())
                            };
                            if let Some(handle) = handle {
                                let _ = handle
                                    .send(command::PermissionQuery { channel_id: id })
                                    .await;
                            }
                        });
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
                let mut dm_unreads_changed = false;
                let mut group_unreads_changed = false;
                let mut dm_sender: Option<u32> = None;
                let mut group_msg_id: Option<String> = None;

                // A message is a DM when it targets specific sessions and has
                // no channel_id.  The Mumble server sets `tm.session` to the
                // list of targeted user sessions (for the recipient) and
                // `tm.channel_id` is empty.
                let is_dm = !tm.session.is_empty() && tm.channel_id.is_empty();

                // Check for a group chat marker before treating as a plain DM.
                // Format: <!-- FANCY_GROUP:uuid -->body
                let group_marker = if is_dm {
                    const PREFIX: &str = "<!-- FANCY_GROUP:";
                    const SUFFIX: &str = " -->";
                    let msg = &tm.message;
                    if let Some(rest) = msg.strip_prefix(PREFIX) {
                        rest.find(SUFFIX).map(|end| {
                            let gid = rest[..end].to_string();
                            let body_start = PREFIX.len() + end + SUFFIX.len();
                            let body = msg[body_start..].to_string();
                            (gid, body)
                        })
                    } else {
                        None
                    }
                } else {
                    None
                };

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

                    if let Some((ref gid, ref stripped_body)) = group_marker {
                        // Group message - route to the correct group conversation.
                        if state.group_chats.contains_key(gid) {
                            let mut msg = ChatMessage {
                                    sender_session: actor,
                                    sender_name,
                                    body: stripped_body.clone(),
                                    channel_id: 0,
                                    is_own: false,
                                    dm_session: None,
                                    group_id: Some(gid.clone()),
                                    message_id: tm.message_id.clone(),
                                    timestamp: tm.timestamp,
                                };
                            msg.ensure_id();
                            state
                                .group_messages
                                .entry(gid.clone())
                                .or_default()
                                .push(msg);

                            if state.selected_group.as_deref() != Some(gid) {
                                *state.group_unread_counts.entry(gid.clone()).or_insert(0) += 1;
                                group_unreads_changed = true;
                            }

                            group_msg_id = Some(gid.clone());
                        }
                    } else if is_dm {
                        let body = tm.message.clone();
                        // Direct message - store keyed by the sender's session.
                        if let Some(sender_session) = actor {
                            let mut msg = ChatMessage {
                                    sender_session: actor,
                                    sender_name,
                                    body,
                                    channel_id: 0,
                                    is_own: false,
                                    dm_session: Some(sender_session),
                                    group_id: None,
                                    message_id: tm.message_id.clone(),
                                    timestamp: tm.timestamp,
                                };
                            msg.ensure_id();
                            state
                                .dm_messages
                                .entry(sender_session)
                                .or_default()
                                .push(msg);

                            // Increment DM unread if this DM conversation is not viewed.
                            if state.selected_dm_user != Some(sender_session) {
                                *state.dm_unread_counts.entry(sender_session).or_insert(0) += 1;
                                dm_unreads_changed = true;
                            }

                            dm_sender = Some(sender_session);
                        }
                    } else {
                        // Channel message - original behaviour.
                        let body = tm.message.clone();
                        let target_channels: Vec<u32> = if tm.channel_id.is_empty() {
                            vec![0]
                        } else {
                            tm.channel_id.clone()
                        };

                        let selected = state.selected_channel;

                        for &ch_id in &target_channels {
                            let mut msg = ChatMessage {
                                    sender_session: actor,
                                    sender_name: sender_name.clone(),
                                    body: body.clone(),
                                    channel_id: ch_id,
                                    is_own: false,
                                    dm_session: None,
                                    group_id: None,
                                    message_id: tm.message_id.clone(),
                                    timestamp: tm.timestamp,
                                };
                            msg.ensure_id();
                            state
                                .messages
                                .entry(ch_id)
                                .or_default()
                                .push(msg);

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
                }

                // Emit group message events outside the lock.
                if let Some(gid) = group_msg_id {
                    let _ = self
                        .app
                        .emit("new-group-message", NewGroupMessagePayload { group_id: gid });

                    // Flash taskbar for group messages.
                    #[cfg(target_os = "windows")]
                    if let Some(window) = self.app.get_webview_window("main") {
                        let _ = window.request_user_attention(Some(
                            tauri::UserAttentionType::Informational,
                        ));
                    }
                }

                if group_unreads_changed {
                    let unreads = self
                        .shared
                        .lock()
                        .map(|s| s.group_unread_counts.clone())
                        .unwrap_or_default();
                    let _ = self
                        .app
                        .emit("group-unread-changed", GroupUnreadPayload { unreads });
                }

                // Emit DM events outside the lock.
                if let Some(sender_session) = dm_sender {
                    let _ = self
                        .app
                        .emit("new-dm", NewDmPayload { session: sender_session });

                    // Always flash taskbar for DMs.
                    #[cfg(target_os = "windows")]
                    if let Some(window) = self.app.get_webview_window("main") {
                        let _ = window.request_user_attention(Some(
                            tauri::UserAttentionType::Informational,
                        ));
                    }
                }

                if dm_unreads_changed {
                    let unreads = self
                        .shared
                        .lock()
                        .map(|s| s.dm_unread_counts.clone())
                        .unwrap_or_default();
                    let _ = self
                        .app
                        .emit("dm-unread-changed", DmUnreadPayload { unreads });
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
                    if let Some(max) = sc.max_users {
                        state.max_users = Some(max);
                    }
                    info!(
                        msg_len = state.server_config.max_message_length,
                        img_len = state.server_config.max_image_message_length,
                        allow_html = state.server_config.allow_html,
                        max_users = ?state.max_users,
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
                    data_len = pd.data.as_ref().map(Vec::len).unwrap_or(0),
                    "plugin data received"
                );

                // Handle group chat creation/updates server-side so the
                // group is known before any group messages arrive.
                if pd.data_id.as_deref() == Some("fancy-group") {
                    if let Some(data) = &pd.data {
                        if let Ok(val) = serde_json::from_slice::<serde_json::Value>(data) {
                            if val.get("action").and_then(|a| a.as_str()) == Some("create") {
                                if let Some(group_val) = val.get("group") {
                                    if let Ok(group) =
                                        serde_json::from_value::<GroupChat>(group_val.clone())
                                    {
                                        info!(group_id = %group.id, name = %group.name, "group chat created via plugin data");
                                        if let Ok(mut state) = self.shared.lock() {
                                            state
                                                .group_chats
                                                .insert(group.id.clone(), group.clone());
                                        }
                                        let _ = self.app.emit(
                                            "group-created",
                                            GroupCreatedPayload { group },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                let _ = self.app.emit(
                    "plugin-data",
                    PluginDataPayload {
                        sender_session: pd.sender_session,
                        data: pd.data.clone().unwrap_or_default(),
                        data_id: pd.data_id.clone().unwrap_or_default(),
                    },
                );
            }

            ControlMessage::PermissionQuery(pq) => {
                info!(
                    channel_id = ?pq.channel_id,
                    permissions = ?pq.permissions,
                    flush = pq.flush(),
                    "permission query response received"
                );

                // If flush is set, clear all cached permissions first.
                if pq.flush() {
                    if let Ok(mut state) = self.shared.lock() {
                        for ch in state.channels.values_mut() {
                            ch.permissions = None;
                        }
                    }
                }

                // Store the permission bitmask on the channel entry.
                if let (Some(channel_id), Some(perms)) = (pq.channel_id, pq.permissions) {
                    if let Ok(mut state) = self.shared.lock() {
                        if let Some(ch) = state.channels.get_mut(&channel_id) {
                            ch.permissions = Some(perms);
                        }
                    }
                    // Notify the frontend that channel data changed.
                    let _ = self.app.emit("state-changed", ());
                }
            }

            ControlMessage::CodecVersion(cv) => {
                if let Ok(mut state) = self.shared.lock() {
                    state.opus = cv.opus();
                }
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
            #[cfg(not(target_os = "android"))]
            {
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
            // Stop audio pipelines on disconnect (desktop only).
            #[cfg(not(target_os = "android"))]
            {
                if let Some(handle) = state.outbound_task_handle.take() {
                    handle.abort();
                }
                state.inbound_pipeline = None;
            }
            state.voice_state = VoiceState::Inactive;
            state.server_fancy_version = None;
            state.server_version_info = ServerVersionInfo::default();
            state.max_users = None;
            state.max_bandwidth = None;
            state.opus = false;
        }
        let _ = self.app.emit("server-disconnected", ());
    }
}
