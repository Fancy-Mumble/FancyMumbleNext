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
use tauri_plugin_notification::NotificationExt;
use tracing::{debug, info, warn};

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

    fn send_notification(&self, title: &str, body: &str) {
        self.send_notification_with_icon(title, body, None, None);
    }

    fn send_notification_with_icon(
        &self,
        title: &str,
        body: &str,
        icon: Option<&[u8]>,
        channel_id: Option<u32>,
    ) {
        // On Android, route through our ConnectionServicePlugin so we can
        // decode the sender avatar as a Bitmap for the notification large-icon.
        #[cfg(target_os = "android")]
        {
            use tauri::Manager;
            if let Some(cs_handle) = self
                .app
                .try_state::<crate::connection_service::ConnectionServiceHandle>()
            {
                crate::connection_service::show_chat_notification(
                    &cs_handle,
                    title,
                    body,
                    icon,
                    channel_id,
                );
                return;
            }
        }
        // Non-Android fallback: standard Tauri notification API (no avatar).
        let _ = icon;
        let _ = channel_id;
        let _ = self
            .app
            .notification()
            .builder()
            .channel_id("messages")
            .title(title)
            .body(body)
            .show();
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
    /// Running count of inbound audio packets (for periodic diagnostics).
    pub(super) inbound_audio_count: u64,
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
            let session = audio.sender_session;
            let is_terminator = audio.is_terminator;

            self.inbound_audio_count += 1;
            if self.inbound_audio_count == 1 || self.inbound_audio_count.is_multiple_of(500) {
                debug!(
                    "inbound audio #{} from session {} (opus {} bytes, seq {}, term={})",
                    self.inbound_audio_count,
                    session,
                    audio.opus_data.len(),
                    audio.frame_number,
                    is_terminator,
                );
            }

            let packet = EncodedPacket {
                data: audio.opus_data.clone(),
                sequence: audio.frame_number,
                frame_samples: 960,
            };
            if let Ok(mut state) = self.shared.lock() {
                let mixer_active = if let Some(ref mut mixer) = state.audio_mixer {
                    if let Err(e) = mixer.feed(session, &packet) {
                        warn!("inbound audio decode error: {e}");
                    }
                    true
                } else {
                    false
                };

                // Only track talking state when the mixer is active.
                // When deafened (mixer is None), ignore incoming audio
                // so stale indicators are never created.
                if mixer_active {
                    if is_terminator {
                        if state.talking_sessions.remove(&session) {
                            let _ = self.app.emit("user-talking", (session, false));
                        }
                    } else if state.talking_sessions.insert(session) {
                        let _ = self.app.emit("user-talking", (session, true));
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
                debug!(
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
            if let Some(mut playback) = state.mixing_playback.take() {
                let _ = playback.stop();
            }
            state.audio_mixer = None;
            state.voice_state = VoiceState::Inactive;
            state.talking_sessions.clear();
            state.server_fancy_version = None;
            state.server_version_info = ServerVersionInfo::default();
            state.max_users = None;
            state.max_bandwidth = None;
            state.opus = false;
            state.root_permissions = None;
            // Save signal state before dropping pchat.
            if let Some(ref pchat) = state.pchat {
                super::pchat::save_signal_state(pchat);
                super::pchat::save_local_cache(pchat);
            }
            state.pchat = None;
            state.pchat_seed = None;
            state.pchat_identity_dir = None;
            state.pending_key_shares.clear();
            user_initiated = state.user_initiated_disconnect;
            state.user_initiated_disconnect = false;
        }
        let reason = if user_initiated { None } else { Some("Connection to server was lost.") };
        let _ = self.app.emit("server-disconnected", reason);

        // Stop Android foreground service now that we are disconnected.
        #[cfg(target_os = "android")]
        {
            use tauri::Manager;
            if let Some(handle) =
                self.app.try_state::<crate::connection_service::ConnectionServiceHandle>()
            {
                crate::connection_service::stop_service(&handle);
            }

            // Keep FCM topic subscriptions active after disconnect so the
            // device continues to receive push notifications while offline.
            // Subscriptions are idempotent — re-subscribing on the next
            // connect is harmless.
        }
    }

    fn on_audio_transport_changed(&mut self, udp_active: bool) {
        info!(udp_active, "audio transport changed");
        let _ = self
            .app
            .emit("audio-transport-changed", udp_active);
    }
}
