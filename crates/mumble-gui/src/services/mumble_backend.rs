//! Real backend that connects to a Mumble server via `mumble-protocol`.
//!
//! Implements [`MumbleService`] by delegating to the async protocol client.
//! A [`GuiEventHandler`] bridges server events into a channel the GUI
//! can consume reactively.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tracing::info;

use mumble_protocol::client::{ClientConfig, ClientHandle};
use mumble_protocol::command;
use mumble_protocol::event::EventHandler;
use mumble_protocol::message::ControlMessage;
use mumble_protocol::transport::tcp::TcpConfig;
use mumble_protocol::transport::udp::UdpConfig;

use super::{
    BoxFuture, ChannelEntry, ChatMessage, ConnectionStatus, MumbleService,
    ServerEvent, ServiceResult, UserEntry,
};

// ─── Shared interior state ────────────────────────────────────────

#[derive(Default)]
struct SharedState {
    status: ConnectionStatus,
    client_handle: Option<ClientHandle>,
    users: HashMap<u32, UserEntry>,
    channels: HashMap<u32, ChannelEntry>,
    /// `channel_id` → messages
    messages: HashMap<u32, Vec<ChatMessage>>,
    own_session: Option<u32>,
    own_name: String,
}

// ─── Event handler bridge ─────────────────────────────────────────

/// Implements `EventHandler` to receive protocol events and forward
/// them to the GUI via a channel + shared state.
struct GuiEventHandler {
    shared: Arc<Mutex<SharedState>>,
    event_tx: mpsc::UnboundedSender<ServerEvent>,
}

impl EventHandler for GuiEventHandler {
    fn on_control_message(&mut self, msg: &ControlMessage) {
        match msg {
            ControlMessage::ServerSync(sync) => {
                if let Ok(mut state) = self.shared.lock() {
                    state.status = ConnectionStatus::Connected;
                    state.own_session = sync.session;
                }
                let _ = self.event_tx.send(ServerEvent::Connected);
            }

            ControlMessage::UserState(us) => {
                if let Some(session) = us.session {
                    if let Ok(mut state) = self.shared.lock() {
                        let user = state
                            .users
                            .entry(session)
                            .or_insert_with(|| UserEntry {
                                session,
                                name: String::new(),
                                channel_id: 0,
                                avatar: None,
                            });
                        if let Some(ref name) = us.name {
                            user.name = name.clone();
                        }
                        if let Some(ch) = us.channel_id {
                            user.channel_id = ch;
                        }
                    }
                    let _ = self.event_tx.send(ServerEvent::StateChanged);
                }
            }

            ControlMessage::UserRemove(ur) => {
                if let Ok(mut state) = self.shared.lock() {
                    state.users.remove(&ur.session);
                }
                let _ = self.event_tx.send(ServerEvent::StateChanged);
            }

            ControlMessage::ChannelState(cs) => {
                if let Some(id) = cs.channel_id {
                    if let Ok(mut state) = self.shared.lock() {
                        let ch = state
                            .channels
                            .entry(id)
                            .or_insert_with(|| ChannelEntry {
                                id,
                                name: String::new(),
                                description: String::new(),
                                user_count: 0,
                            });
                        if let Some(ref name) = cs.name {
                            ch.name = name.clone();
                        }
                        if let Some(ref desc) = cs.description {
                            ch.description = desc.clone();
                        }
                    }
                    let _ = self.event_tx.send(ServerEvent::StateChanged);
                }
            }

            ControlMessage::ChannelRemove(cr) => {
                if let Ok(mut state) = self.shared.lock() {
                    state.channels.remove(&cr.channel_id);
                    state.messages.remove(&cr.channel_id);
                }
                let _ = self.event_tx.send(ServerEvent::StateChanged);
            }

            ControlMessage::TextMessage(tm) => {
                if let Ok(mut state) = self.shared.lock() {
                    let actor = tm.actor;
                    let own_session = state.own_session;

                    // Don't duplicate messages we sent ourselves
                    let is_own = actor == own_session && actor.is_some();
                    if is_own {
                        return;
                    }

                    let sender_name = actor
                        .and_then(|sid| state.users.get(&sid))
                        .map(|u| u.name.clone())
                        .unwrap_or_else(|| "Server".into());

                    let body = tm.message.clone();

                    // Add to each target channel
                    let target_channels: Vec<u32> = if tm.channel_id.is_empty() {
                        // If no channel specified, try channel 0 (Root)
                        vec![0]
                    } else {
                        tm.channel_id.clone()
                    };

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
                        let _ = self.event_tx.send(ServerEvent::NewMessage {
                            channel_id: ch_id,
                        });
                    }
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
                }
                let _ = self.event_tx.send(ServerEvent::Rejected { reason });
            }

            _ => {}
        }
    }

    fn on_connected(&mut self) {
        info!("protocol: connected (ServerSync received)");
    }

    fn on_disconnected(&mut self) {
        if let Ok(mut state) = self.shared.lock() {
            state.status = ConnectionStatus::Disconnected;
            state.client_handle = None;
        }
        let _ = self.event_tx.send(ServerEvent::Disconnected);
    }
}

// ─── Concrete backend ─────────────────────────────────────────────

/// The real Mumble backend used by the GUI.
#[derive(Clone)]
pub struct MumbleBackend {
    shared: Arc<Mutex<SharedState>>,
    event_tx: mpsc::UnboundedSender<ServerEvent>,
    /// Taken once by the GUI event bridge coroutine.
    event_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<ServerEvent>>>>,
}

impl MumbleBackend {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            shared: Arc::new(Mutex::new(SharedState::default())),
            event_tx: tx,
            event_rx: Arc::new(Mutex::new(Some(rx))),
        }
    }

    /// Take the event receiver - call exactly once from the GUI coroutine.
    pub fn take_event_receiver(&self) -> Option<mpsc::UnboundedReceiver<ServerEvent>> {
        self.event_rx
            .lock()
            .ok()
            .and_then(|mut guard| guard.take())
    }

    /// Recompute `user_count` for every channel based on current users.
    fn refresh_user_counts(state: &mut SharedState) {
        // Reset counts
        for ch in state.channels.values_mut() {
            ch.user_count = 0;
        }
        // Tally
        for user in state.users.values() {
            if let Some(ch) = state.channels.get_mut(&user.channel_id) {
                ch.user_count += 1;
            }
        }
    }
}

impl MumbleService for MumbleBackend {
    fn connect(
        &self,
        host: String,
        port: u16,
        username: String,
    ) -> BoxFuture<'_, ServiceResult<()>> {
        let shared = self.shared.clone();
        let event_tx = self.event_tx.clone();

        Box::pin(async move {
            // Update status
            if let Ok(mut state) = shared.lock() {
                state.status = ConnectionStatus::Connecting;
                state.own_name = username.clone();
                // Clear stale data
                state.users.clear();
                state.channels.clear();
                state.messages.clear();
                state.own_session = None;
                state.client_handle = None;
            }

            let config = ClientConfig {
                tcp: TcpConfig {
                    server_host: host.clone(),
                    server_port: port,
                    accept_invalid_certs: true,
                },
                udp: UdpConfig {
                    server_host: host,
                    server_port: port,
                },
                ..ClientConfig::default()
            };

            let handler = GuiEventHandler {
                shared: shared.clone(),
                event_tx,
            };

            let (handle, _join) = mumble_protocol::client::run(config, handler)
                .await
                .map_err(|e| format!("Connection failed: {e}"))?;

            // Store the handle
            if let Ok(mut state) = shared.lock() {
                state.client_handle = Some(handle.clone());
            }

            // Authenticate
            handle
                .send(command::Authenticate {
                    username,
                    password: None,
                    tokens: vec![],
                })
                .await
                .map_err(|e| format!("Failed to send auth: {e}"))?;

            info!("TCP connected, authenticate sent - waiting for ServerSync");
            Ok(())
        })
    }

    fn disconnect(&self) -> BoxFuture<'_, ServiceResult<()>> {
        let shared = self.shared.clone();
        Box::pin(async move {
            let handle = shared
                .lock()
                .ok()
                .and_then(|s| s.client_handle.clone());

            if let Some(handle) = handle {
                let _ = handle.send(command::Disconnect).await;
            }

            if let Ok(mut state) = shared.lock() {
                state.status = ConnectionStatus::Disconnected;
                state.client_handle = None;
                state.users.clear();
                state.channels.clear();
                state.messages.clear();
                state.own_session = None;
            }
            Ok(())
        })
    }

    fn status(&self) -> ConnectionStatus {
        self.shared
            .lock()
            .map(|s| s.status)
            .unwrap_or(ConnectionStatus::Disconnected)
    }

    fn channels(&self) -> Vec<ChannelEntry> {
        self.shared
            .lock()
            .map(|mut s| {
                Self::refresh_user_counts(&mut s);
                let mut channels: Vec<_> = s.channels.values().cloned().collect();
                channels.sort_by_key(|c| c.id);
                channels
            })
            .unwrap_or_default()
    }

    fn users(&self) -> Vec<UserEntry> {
        self.shared
            .lock()
            .map(|s| s.users.values().cloned().collect())
            .unwrap_or_default()
    }

    fn send_message(
        &self,
        channel_id: u32,
        body: String,
    ) -> BoxFuture<'_, ServiceResult<()>> {
        let shared = self.shared.clone();
        Box::pin(async move {
            let (handle, own_session, own_name) = {
                let state = shared.lock().map_err(|e| e.to_string())?;
                (
                    state.client_handle.clone(),
                    state.own_session,
                    state.own_name.clone(),
                )
            };

            let handle = handle.ok_or("Not connected")?;

            // Send the text message command
            handle
                .send(command::SendTextMessage {
                    channel_ids: vec![channel_id],
                    user_sessions: vec![],
                    tree_ids: vec![],
                    message: body.clone(),
                })
                .await
                .map_err(|e| format!("Failed to send message: {e}"))?;

            // Add locally (server does not echo our own messages back)
            if let Ok(mut state) = shared.lock() {
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
        })
    }

    fn messages(&self, channel_id: u32) -> Vec<ChatMessage> {
        self.shared
            .lock()
            .map(|s| s.messages.get(&channel_id).cloned().unwrap_or_default())
            .unwrap_or_default()
    }
}
