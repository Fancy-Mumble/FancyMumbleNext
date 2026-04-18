use mumble_protocol::proto::mumble_tcp;
use tracing::info;

use super::{HandleMessage, HandlerContext};
use crate::state::types::*;

impl HandleMessage for mumble_tcp::UserRemove {
    fn handle(&self, ctx: &HandlerContext) {
        let is_self_kicked = {
            let state = ctx.shared.lock().ok();
            state.and_then(|s| s.conn.own_session) == Some(self.session)
        };

        if is_self_kicked {
            // We got kicked/banned - clean up and notify frontend.
            let reason = self
                .reason
                .clone()
                .unwrap_or_else(|| "Disconnected by server".into());
            info!("Kicked from server: {reason}");
            if let Ok(mut state) = ctx.shared.lock() {
                state.conn.status = ConnectionStatus::Disconnected;
                state.conn.client_handle = None;
                state.conn.event_loop_handle = None;
                state.users.clear();
                state.channels.clear();
                state.msgs.by_channel.clear();
                state.conn.own_session = None;
                state.conn.synced = false;
                state.permanently_listened.clear();
                state.selected_channel = None;
                state.current_channel = None;
                state.msgs.channel_unread.clear();
                state.server.config = ServerConfig::default();
                state.audio.voice_state = VoiceState::Inactive;
                state.server.fancy_version = None;
                state.server.version_info = ServerVersionInfo::default();
                state.server.max_users = None;
                state.server.max_bandwidth = None;
                state.server.opus = false;
                // Stop audio pipelines (desktop only).
                #[cfg(not(target_os = "android"))]
                stop_audio_pipelines(&mut state);
            }
            ctx.emit("connection-rejected", RejectedPayload { reason, reject_type: None });
            ctx.emit_empty("server-disconnected");
        } else {
            // Look up the departing user's name for the activity log.
            let departing_name = ctx
                .shared
                .lock()
                .ok()
                .and_then(|s| s.users.get(&self.session).map(|u| u.name.clone()));

            let deferred_share_events: Vec<(u32, Vec<PendingKeyShare>)> =
                if let Ok(mut state) = ctx.shared.lock() {
                    // Look up the departing user's cert hash before removing them.
                    let cert_hash = state
                        .users
                        .get(&self.session)
                        .and_then(|u| u.hash.clone());

                    let _ = state.users.remove(&self.session);

                    // Remove any pending key-share requests from the departing user.
                    if let Some(ref hash) = cert_hash {
                        let before_len = state.pchat_ctx.pending_key_shares.len();
                        let removed: Vec<_> = state
                            .pchat_ctx.pending_key_shares
                            .iter()
                            .filter(|p| p.peer_cert_hash == *hash)
                            .map(|p| p.channel_id)
                            .collect();
                        state
                            .pchat_ctx.pending_key_shares
                            .retain(|p| p.peer_cert_hash != *hash);
                        collect_affected_channels(&state, before_len, removed)
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };

            // Emit outside the lock to avoid deadlock with Tauri IPC.
            for (ch_id, remaining) in deferred_share_events {
                ctx.emit(
                    "pchat-key-share-requests-changed",
                    KeyShareRequestsChangedPayload {
                        channel_id: ch_id,
                        pending: remaining,
                    },
                );
            }
            ctx.emit_empty("state-changed");

            if let Some(name) = departing_name {
                if !name.is_empty() {
                    ctx.emit(
                        "server-log",
                        ServerLogEntry::now(format!("{name} disconnected")),
                    );
                }
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
fn stop_audio_pipelines(state: &mut crate::state::SharedState) {
    if let Some(handle) = state.audio.outbound_task_handle.take() {
        handle.abort();
    }
    if let Some(mut playback) = state.audio.mixing_playback.take() {
        let _ = playback.stop();
    }
    state.audio.mixer = None;
}

fn collect_affected_channels(
    state: &crate::state::SharedState,
    before_len: usize,
    removed: Vec<u32>,
) -> Vec<(u32, Vec<PendingKeyShare>)> {
    if state.pchat_ctx.pending_key_shares.len() == before_len {
        return Vec::new();
    }
    let affected_channels: std::collections::HashSet<u32> = removed.into_iter().collect();
    affected_channels
        .into_iter()
        .map(|ch_id| {
            let remaining: Vec<_> = state
                .pchat_ctx.pending_key_shares
                .iter()
                .filter(|p| p.channel_id == ch_id)
                .cloned()
                .collect();
            (ch_id, remaining)
        })
        .collect()
}
