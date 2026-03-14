use mumble_protocol::proto::mumble_tcp;
use tracing::info;

use super::{HandleMessage, HandlerContext};
use crate::state::types::*;

impl HandleMessage for mumble_tcp::UserRemove {
    fn handle(&self, ctx: &HandlerContext) {
        let is_self_kicked = {
            let state = ctx.shared.lock().ok();
            state.and_then(|s| s.own_session) == Some(self.session)
        };

        if is_self_kicked {
            // We got kicked/banned - clean up and notify frontend.
            let reason = self
                .reason
                .clone()
                .unwrap_or_else(|| "Disconnected by server".into());
            info!("Kicked from server: {reason}");
            if let Ok(mut state) = ctx.shared.lock() {
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
            ctx.emit("connection-rejected", RejectedPayload { reason });
            ctx.emit_empty("server-disconnected");
        } else {
            if let Ok(mut state) = ctx.shared.lock() {
                state.users.remove(&self.session);
            }
            ctx.emit_empty("state-changed");
        }
    }
}
