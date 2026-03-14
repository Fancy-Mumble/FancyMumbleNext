use std::sync::Arc;

use mumble_protocol::command;
use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::ChannelEntry;

impl HandleMessage for mumble_tcp::ChannelState {
    fn handle(&self, ctx: &HandlerContext) {
        let Some(id) = self.channel_id else { return };

        let (is_synced, needs_description) = {
            let mut state_guard = ctx.shared.lock().ok();
            if let Some(ref mut state) = state_guard {
                let ch = state.channels.entry(id).or_insert_with(|| ChannelEntry {
                    id,
                    parent_id: None,
                    name: String::new(),
                    description: String::new(),
                    description_hash: None,
                    user_count: 0,
                    permissions: None,
                });
                if let Some(parent) = self.parent {
                    ch.parent_id = Some(parent);
                }
                if let Some(ref name) = self.name {
                    ch.name = name.clone();
                }
                if let Some(ref desc) = self.description {
                    ch.description = desc.clone();
                }
                if let Some(ref hash) = self.description_hash {
                    ch.description_hash = Some(hash.clone());
                }
                let needs_desc =
                    ch.description.is_empty() && ch.description_hash.is_some() && state.synced;
                (state.synced, needs_desc)
            } else {
                (false, false)
            }
        };

        // Request the full description blob if only a hash
        // was provided (large descriptions are deferred).
        if needs_description {
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
                            channel_description: vec![id],
                        })
                        .await;
                }
            });
        }

        // When a channel state changes, re-query its permissions
        // so the cached bitmask stays up-to-date (ACL changes, etc.).
        if is_synced {
            let shared = Arc::clone(&ctx.shared);
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
            ctx.emit_empty("state-changed");
        }
    }
}
