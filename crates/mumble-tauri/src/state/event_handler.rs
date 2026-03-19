//! `EventHandler` implementation that bridges mumble-protocol events
//! to the React frontend via Tauri's event system.
//!
//! Control message handling is delegated to the `handler` module,
//! where each protobuf message type has its own file implementing
//! `HandleMessage`.  This file retains `on_connected`,
//! `on_udp_message`, and `on_disconnected` which are thin wrappers.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};
#[cfg(target_os = "windows")]
use tauri::Manager;
use tracing::info;
#[cfg(not(target_os = "android"))]
use tracing::warn;

#[cfg(not(target_os = "android"))]
use mumble_protocol::audio::encoder::EncodedPacket;
use mumble_protocol::event::EventHandler;
use mumble_protocol::message::{ControlMessage, UdpMessage};

use super::handler::{self, EventEmitter, HandlerContext};
use super::types::*;
use super::SharedState;

/// Tauri-backed event emitter forwarding to `AppHandle::emit`.
struct TauriEmitter {
    app: AppHandle,
}

impl EventEmitter for TauriEmitter {
    fn emit_json(&self, event: &str, payload: serde_json::Value) {
        let _ = self.app.emit(event, payload);
    }

    fn request_user_attention(&self) {
        #[cfg(target_os = "windows")]
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.request_user_attention(Some(
                tauri::UserAttentionType::Informational,
            ));
        }
    }
}

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
        let ctx = HandlerContext {
            shared: Arc::clone(&self.shared),
            emitter: Box::new(TauriEmitter {
                app: self.app.clone(),
            }),
        };
        handler::dispatch(msg, &ctx);
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
        let mut user_initiated = false;
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
            state.pchat = None;
            state.pchat_seed = None;
            state.pchat_identity_dir = None;
            state.pending_key_shares.clear();
            user_initiated = state.user_initiated_disconnect;
            state.user_initiated_disconnect = false;
        }
        let reason = if user_initiated { None } else { Some("Connection to server was lost.") };
        let _ = self.app.emit("server-disconnected", reason);
    }
}
