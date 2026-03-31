use mumble_protocol::proto::mumble_tcp;
use tracing::debug;

use super::{HandleMessage, HandlerContext};
use crate::state::pchat;
use crate::state::types::{
    KeyHoldersChangedPayload, NewMessagePayload, PchatFetchCompletePayload,
    PchatHistoryLoadingPayload, ReactionDeliverPayload, ReactionFetchResponsePayload,
    StoredReactionPayload,
};

impl HandleMessage for mumble_tcp::PchatMessageDeliver {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatMessageDeliver");
        let channel_id = self.channel_id.unwrap_or(0);
        pchat::handle_proto_msg_deliver(&ctx.shared, self);
        ctx.emit("new-message", NewMessagePayload { channel_id });
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatFetchResponse {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatFetchResponse");
        let channel_id = self.channel_id.unwrap_or(0);
        let has_more = self.has_more.unwrap_or(false);
        let total_stored = self.total_stored.unwrap_or(0);
        pchat::handle_proto_fetch_resp(&ctx.shared, self);
        // Signal that history loading is complete for this channel.
        ctx.emit("pchat-history-loading", PchatHistoryLoadingPayload { channel_id, loading: false });
        ctx.emit("pchat-fetch-complete", PchatFetchCompletePayload { channel_id, has_more, total_stored });
        ctx.emit("new-message", NewMessagePayload { channel_id });
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatKeyAnnounce {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatKeyAnnounce");
        pchat::handle_proto_key_announce(&ctx.shared, self);
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatKeyExchange {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatKeyExchange");
        pchat::handle_proto_key_exchange(&ctx.shared, self);
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatKeyRequest {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatKeyRequest");
        pchat::handle_proto_key_request(&ctx.shared, self);
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatAck {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatAck");

        let status = self.status.unwrap_or(0);
        let is_deleted = status == mumble_tcp::PchatAckStatus::PchatAckDeleted as i32;
        let is_rejected = status == mumble_tcp::PchatAckStatus::PchatAckRejected as i32
            || status == mumble_tcp::PchatAckStatus::PchatAckQuotaExceeded as i32;

        // If a delete request is pending, resolve its oneshot channel.
        if is_deleted || is_rejected {
            if let Ok(mut state) = ctx.shared.lock() {
                if let Some(tx) = state.pending_delete_ack.take() {
                    let _ = tx.send(crate::state::types::DeleteAckResult {
                        success: is_deleted,
                        reason: self.reason.clone(),
                    });
                }
            }
        }

        pchat::handle_proto_ack(self);
    }
}

impl HandleMessage for mumble_tcp::PchatKeyHoldersList {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatKeyHoldersList");
        let channel_id = self.channel_id.unwrap_or(0);

        let holders = {
            let Ok(mut state) = ctx.shared.lock() else {
                return;
            };

            // Build entries, resolving online status from known users.
            let online_hashes: std::collections::HashSet<&str> = state
                .users
                .values()
                .filter_map(|u| u.hash.as_deref())
                .collect();

            let holders: Vec<_> = self
                .holders
                .iter()
                .map(|entry| {
                    let cert_hash = entry.cert_hash.clone().unwrap_or_default();
                    // Prefer name from online user, fall back to server-provided name,
                    // then stored name from the hash resolver, and finally a
                    // deterministic human-readable name generated from the hash.
                    let online_name = state
                        .users
                        .values()
                        .find(|u| u.hash.as_deref() == Some(&cert_hash))
                        .map(|u| u.name.clone());
                    let name = online_name.unwrap_or_else(|| {
                        let server_name = entry.name.clone().unwrap_or_default();
                        if !server_name.is_empty() && server_name != cert_hash {
                            return server_name;
                        }
                        if let Some(ref resolver) = state.hash_name_resolver {
                            resolver.resolve(&cert_hash)
                        } else {
                            cert_hash.clone()
                        }
                    });
                    let is_online = online_hashes.contains(cert_hash.as_str());
                    crate::state::types::KeyHolderEntry {
                        cert_hash,
                        name,
                        is_online,
                    }
                })
                .collect();

            let _ = state.key_holders.insert(channel_id, holders.clone());

            // Sync server-provided holder list into key_manager so that
            // consent checks can skip peers who already hold the key.
            let holder_hashes: std::collections::HashSet<&str> = holders
                .iter()
                .filter(|e| !e.cert_hash.is_empty())
                .map(|e| e.cert_hash.as_str())
                .collect();

            if let Some(ref mut pchat) = state.pchat {
                pchat.key_manager.replace_key_holders(
                    channel_id,
                    holder_hashes.iter().map(|h| (*h).to_owned()).collect(),
                );
            }

            // Remove any pending consent prompts for peers the server now
            // confirms as holders -- they already have the key.
            let before_len = state.pending_key_shares.len();
            state.pending_key_shares.retain(|p| {
                !(p.channel_id == channel_id && holder_hashes.contains(p.peer_cert_hash.as_str()))
            });

            // Notify the frontend so it drops the stale "Share Key" banner.
            if state.pending_key_shares.len() != before_len {
                if let Some(ref app) = state.tauri_app_handle {
                    use tauri::Emitter;
                    let remaining: Vec<_> = state
                        .pending_key_shares
                        .iter()
                        .filter(|p| p.channel_id == channel_id)
                        .cloned()
                        .collect();
                    let _ = app.emit(
                        "pchat-key-share-requests-changed",
                        crate::state::types::KeyShareRequestsChangedPayload {
                            channel_id,
                            pending: remaining,
                        },
                    );
                }
            }

            holders
        };

        ctx.emit(
            "pchat-key-holders-changed",
            KeyHoldersChangedPayload {
                channel_id,
                holders,
            },
        );
    }
}

impl HandleMessage for mumble_tcp::PchatKeyChallenge {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatKeyChallenge");
        pchat::handle_proto_key_challenge(&ctx.shared, self);
    }
}

impl HandleMessage for mumble_tcp::PchatKeyChallengeResult {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatKeyChallengeResult");
        pchat::handle_proto_key_challenge_result(&ctx.shared, self);
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatDeleteMessages {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatDeleteMessages");
        let channel_id = self.channel_id.unwrap_or(0);
        pchat::handle_proto_delete_messages(&ctx.shared, self);
        ctx.emit("new-message", NewMessagePayload { channel_id });
    }
}

impl HandleMessage for mumble_tcp::PchatOfflineQueueDrain {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatOfflineQueueDrain");
        let channel_id = self.channel_id.unwrap_or(0);
        pchat::handle_proto_offline_queue_drain(&ctx.shared, self);
        ctx.emit("new-message", NewMessagePayload { channel_id });
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatReactionDeliver {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatReactionDeliver");
        let channel_id = self.channel_id.unwrap_or(0);
        let message_id = self.message_id.clone().unwrap_or_default();
        let action = self.action.unwrap_or(0);
        let sender_hash = self.sender_hash.clone().unwrap_or_default();
        let sender_name = self.sender_name.clone().unwrap_or_default();
        let timestamp = self.timestamp.unwrap_or(0);

        // Resolve the emoji string from the oneof.
        let emoji = match &self.emoji {
            Some(mumble_tcp::pchat_reaction_deliver::Emoji::UnicodeEmoji(u)) => {
                u.grapheme.clone().unwrap_or_default()
            }
            Some(mumble_tcp::pchat_reaction_deliver::Emoji::ServerEmoji(s)) => {
                // Reconstruct shortcode as ":name:" for display.
                let bytes = s.shortcode.clone().unwrap_or_default();
                let code = String::from_utf8_lossy(&bytes);
                format!(":{code}:")
            }
            None => String::new(),
        };

        let action_str = if action == mumble_tcp::ReactionAction::ReactionRemove as i32 {
            "remove"
        } else {
            "add"
        };

        ctx.emit(
            "pchat-reaction-deliver",
            ReactionDeliverPayload {
                channel_id,
                message_id,
                emoji,
                action: action_str.to_owned(),
                sender_hash,
                sender_name,
                timestamp,
            },
        );
    }
}

impl HandleMessage for mumble_tcp::PchatReactionFetchResponse {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatReactionFetchResponse");
        let channel_id = self.channel_id.unwrap_or(0);

        let reactions: Vec<StoredReactionPayload> = self
            .reactions
            .iter()
            .map(|r| {
                let emoji = match &r.emoji {
                    Some(
                        mumble_tcp::pchat_reaction_fetch_response::stored_reaction::Emoji::UnicodeEmoji(u),
                    ) => u.grapheme.clone().unwrap_or_default(),
                    Some(
                        mumble_tcp::pchat_reaction_fetch_response::stored_reaction::Emoji::ServerEmoji(s),
                    ) => {
                        let bytes = s.shortcode.clone().unwrap_or_default();
                        let code = String::from_utf8_lossy(&bytes);
                        format!(":{code}:")
                    }
                    None => String::new(),
                };
                StoredReactionPayload {
                    message_id: r.message_id.clone().unwrap_or_default(),
                    emoji,
                    sender_hash: r.sender_hash.clone().unwrap_or_default(),
                    sender_name: r.sender_name.clone().unwrap_or_default(),
                    timestamp: r.timestamp.unwrap_or(0),
                }
            })
            .collect();

        ctx.emit(
            "pchat-reaction-fetch-response",
            ReactionFetchResponsePayload {
                channel_id,
                reactions,
            },
        );
    }
}
