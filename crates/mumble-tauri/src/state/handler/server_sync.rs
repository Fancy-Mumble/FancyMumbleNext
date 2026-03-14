use std::sync::Arc;

use mumble_protocol::command;
use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::{ConnectionStatus, CurrentChannelPayload};

impl HandleMessage for mumble_tcp::ServerSync {
    fn handle(&self, ctx: &HandlerContext) {
        let sessions: Vec<u32>;
        let initial_channel: Option<u32>;
        {
            let Ok(mut state) = ctx.shared.lock() else {
                return;
            };
            state.status = ConnectionStatus::Connected;
            state.own_session = self.session;
            state.synced = true;
            state.max_bandwidth = self.max_bandwidth;
            state.welcome_text = self.welcome_text.clone();

            // Now that we know our session, look up the channel
            // from UserState messages that arrived before ServerSync.
            initial_channel = self
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
        ctx.emit_empty("server-connected");

        // Notify frontend about the initial channel assignment.
        if let Some(ch) = initial_channel {
            ctx.emit(
                "current-channel-changed",
                CurrentChannelPayload { channel_id: ch },
            );
        }

        // Request full texture & comment blobs for every user.
        if !sessions.is_empty() {
            let shared = Arc::clone(&ctx.shared);
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
                            channel_description: Vec::new(),
                        })
                        .await;
                }
            });
        }

        // Request full description blobs for channels that only
        // have a description_hash (the server omits large
        // descriptions during initial sync).
        {
            let channel_ids_needing_desc: Vec<u32>;
            {
                let state = ctx.shared.lock().ok();
                channel_ids_needing_desc = state
                    .map(|s| {
                        s.channels
                            .values()
                            .filter(|ch| {
                                ch.description.is_empty() && ch.description_hash.is_some()
                            })
                            .map(|ch| ch.id)
                            .collect()
                    })
                    .unwrap_or_default();
            }
            if !channel_ids_needing_desc.is_empty() {
                let shared = Arc::clone(&ctx.shared);
                tokio::spawn(async move {
                    let handle = {
                        let state = shared.lock().ok();
                        state.and_then(|s| s.client_handle.clone())
                    };
                    if let Some(handle) = handle {
                        let _ = handle
                            .send(command::RequestBlob {
                                session_texture: Vec::new(),
                                session_comment: Vec::new(),
                                channel_description: channel_ids_needing_desc,
                            })
                            .await;
                    }
                });
            }
        }

        // Request permissions for all known channels so the UI
        // can grey out actions the user is not allowed to perform.
        {
            let channel_ids: Vec<u32>;
            {
                let state = ctx.shared.lock().ok();
                channel_ids = state
                    .map(|s| s.channels.keys().copied().collect())
                    .unwrap_or_default();
            }
            let shared = Arc::clone(&ctx.shared);
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
}
