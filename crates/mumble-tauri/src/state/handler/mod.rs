//! Trait-based message handler dispatch.
//!
//! Each `ControlMessage` variant is handled by a dedicated file that
//! implements `HandleMessage` for the corresponding protobuf struct.
//! This keeps each handler focused and testable in isolation.

mod acl;
mod ban_list;
mod channel_remove;
mod channel_state;
mod codec_version;
mod pchat;
mod permission_denied;
mod permission_query;
mod ping;
mod plugin_data;
mod reject;
mod server_config;
mod server_sync;
mod text_message;
mod user_list;
mod user_remove;
mod user_state;
mod user_stats;
mod version;

#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};

use serde::Serialize;

use mumble_protocol::message::ControlMessage;

use super::SharedState;

/// Abstraction over frontend event emission.
///
/// In production this wraps a `tauri::AppHandle`; in tests it records
/// emitted events for assertion.
pub(crate) trait EventEmitter: Send + Sync {
    /// Emit a serialised event to the frontend.
    fn emit_json(&self, event: &str, payload: serde_json::Value);

    /// Flash the taskbar / request user attention (desktop-only, no-op elsewhere).
    fn request_user_attention(&self);

    /// Send a native OS notification (e.g. Android notification when backgrounded).
    fn send_notification(&self, title: &str, body: &str);
}

/// Context passed to each message handler.
pub(crate) struct HandlerContext {
    pub shared: Arc<Mutex<SharedState>>,
    pub emitter: Box<dyn EventEmitter>,
}

impl HandlerContext {
    /// Emit a typed event payload to the frontend.
    pub fn emit<S: Serialize>(&self, event: &str, payload: S) {
        if let Ok(val) = serde_json::to_value(payload) {
            self.emitter.emit_json(event, val);
        }
    }

    /// Emit an event with an empty (`null`) payload.
    pub fn emit_empty(&self, event: &str) {
        self.emitter.emit_json(event, serde_json::Value::Null);
    }

    /// Flash the taskbar / request user attention.
    pub fn request_user_attention(&self) {
        self.emitter.request_user_attention();
    }

    /// Send a native OS notification (only if notifications are enabled).
    pub fn send_notification(&self, title: &str, body: &str) {
        let enabled = self
            .shared
            .lock()
            .map(|s| s.notifications_enabled)
            .unwrap_or(true);
        if enabled {
            self.emitter.send_notification(title, body);
        }
    }
}

/// Trait for handling a specific control message type.
pub(crate) trait HandleMessage {
    fn handle(&self, ctx: &HandlerContext);
}

/// Dispatch a `ControlMessage` to the appropriate handler.
pub(crate) fn dispatch(msg: &ControlMessage, ctx: &HandlerContext) {
    match msg {
        ControlMessage::Ping(m) => m.handle(ctx),
        ControlMessage::Version(m) => m.handle(ctx),
        ControlMessage::ServerSync(m) => m.handle(ctx),
        ControlMessage::UserState(m) => m.handle(ctx),
        ControlMessage::UserRemove(m) => m.handle(ctx),
        ControlMessage::ChannelState(m) => m.handle(ctx),
        ControlMessage::ChannelRemove(m) => m.handle(ctx),
        ControlMessage::TextMessage(m) => m.handle(ctx),
        ControlMessage::Reject(m) => m.handle(ctx),
        ControlMessage::ServerConfig(m) => m.handle(ctx),
        ControlMessage::PermissionDenied(m) => m.handle(ctx),
        ControlMessage::PluginDataTransmission(m) => m.handle(ctx),
        ControlMessage::PermissionQuery(m) => m.handle(ctx),
        ControlMessage::CodecVersion(m) => m.handle(ctx),
        ControlMessage::UserStats(m) => m.handle(ctx),
        ControlMessage::PchatMessageDeliver(m) => m.handle(ctx),
        ControlMessage::PchatFetchResponse(m) => m.handle(ctx),
        ControlMessage::PchatKeyAnnounce(m) => m.handle(ctx),
        ControlMessage::PchatKeyExchange(m) => m.handle(ctx),
        ControlMessage::PchatKeyRequest(m) => m.handle(ctx),
        ControlMessage::PchatAck(m) => m.handle(ctx),
        ControlMessage::PchatKeyHoldersList(m) => m.handle(ctx),
        ControlMessage::PchatKeyChallenge(m) => m.handle(ctx),
        ControlMessage::PchatKeyChallengeResult(m) => m.handle(ctx),
        ControlMessage::BanList(m) => m.handle(ctx),
        ControlMessage::UserList(m) => m.handle(ctx),
        ControlMessage::Acl(m) => m.handle(ctx),
        _ => {}
    }
}
