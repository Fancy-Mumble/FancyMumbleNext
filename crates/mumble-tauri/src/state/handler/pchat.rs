use std::collections::HashMap;

use fancy_utils::html::strip_html_tags;
use mumble_protocol::proto::mumble_tcp;
use tracing::{debug, warn};

use super::{HandleMessage, HandlerContext};
use crate::state::local_cache::{CachedReaction, LocalMessageCache};
use crate::state::pchat;
use crate::state::types::{
    ChatMessage, KeyHoldersChangedPayload, NewMessagePayload, PchatFetchCompletePayload,
    PchatHistoryLoadingPayload, PinDeliverPayload, PinFetchResponsePayload,
    ReactionDeliverPayload, ReactionFetchResponsePayload, StoredPinPayload,
    StoredReactionPayload, UnreadPayload,
};

impl HandleMessage for mumble_tcp::PchatMessageDeliver {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatMessageDeliver");
        let channel_id = self.channel_id.unwrap_or(0);
        pchat::handle_proto_msg_deliver(&ctx.shared, self);

        let (selected, app_focused, unreads_changed, sender_name, body, sender_session) = {
            let Ok(mut state) = ctx.shared.lock() else {
                warn!("shared state lock poisoned in PchatMessageDeliver");
                return;
            };

            let selected = state.selected_channel;
            let app_focused = state.prefs.app_focused;

            let unreads_changed = if selected != Some(channel_id) {
                *state.msgs.channel_unread.entry(channel_id).or_insert(0) += 1;
                true
            } else {
                false
            };

            let sender_hash = self.sender_hash.as_deref().unwrap_or_default();
            let sender_session = state
                .users
                .values()
                .find(|u| u.hash.as_deref() == Some(sender_hash))
                .map(|u| u.session);
            let sender_name = sender_session
                .and_then(|sid| state.users.get(&sid))
                .map(|u| u.name.clone())
                .unwrap_or_else(|| "Unknown".into());

            let body = state
                .msgs.by_channel
                .get(&channel_id)
                .and_then(|msgs| msgs.last())
                .map(|m| m.body.clone())
                .unwrap_or_default();

            (selected, app_focused, unreads_changed, sender_name, body, sender_session)
        };

        ctx.emit("new-message", NewMessagePayload { channel_id, sender_session });

        if unreads_changed {
            let unreads = ctx
                .shared
                .lock()
                .map(|s| s.msgs.channel_unread.clone())
                .unwrap_or_default();
            ctx.emit("unread-changed", UnreadPayload { unreads });
        }

        if selected != Some(channel_id) || !app_focused {
            let channel_name = ctx
                .shared
                .lock()
                .ok()
                .and_then(|s| s.channels.get(&channel_id).map(|c| c.name.clone()));
            let title = match channel_name {
                Some(name) => format!("{sender_name} in #{name}"),
                None => sender_name,
            };
            let icon = sender_session.and_then(|sid| {
                ctx.shared
                    .lock()
                    .ok()?
                    .users
                    .get(&sid)?
                    .texture
                    .clone()
            });
            ctx.send_notification_with_icon(
                &title,
                &strip_html_tags(&body),
                icon.as_deref(),
                Some(channel_id),
            );
        }

        if selected != Some(channel_id)
            && ctx.shared.lock().is_ok_and(|s| s.permanently_listened.contains(&channel_id))
        {
            ctx.request_user_attention();
        }

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
        ctx.emit("new-message", NewMessagePayload { channel_id, sender_session: None });
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
            let senders = if let Ok(mut state) = ctx.shared.lock() {
                std::mem::take(&mut state.pchat_ctx.pending_delete_acks)
            } else {
                Vec::new()
            };
            for tx in senders {
                let _ = tx.send(crate::state::types::DeleteAckResult {
                    success: is_deleted,
                    reason: self.reason.clone(),
                });
            }
        }

        pchat::handle_proto_ack(self);
    }
}

impl HandleMessage for mumble_tcp::PchatKeyHoldersList {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatKeyHoldersList");
        let channel_id = self.channel_id.unwrap_or(0);

        let (holders, share_requests_payload) = {
            let Ok(mut state) = ctx.shared.lock() else {
                return;
            };

            // Build a single hash -> name map from online users so the
            // per-holder lookup is O(1) instead of O(N) (the previous
            // `state.users.values().find(...)` per holder grew to
            // O(holders * users) and contended with the audio task for
            // the SharedState lock on busy servers).
            let online_name_by_hash: HashMap<&str, &str> = state
                .users
                .values()
                .filter_map(|u| u.hash.as_deref().map(|h| (h, u.name.as_str())))
                .collect();

            let holders: Vec<_> = self
                .holders
                .iter()
                .map(|entry| {
                    let cert_hash = entry.cert_hash.clone().unwrap_or_default();
                    // Prefer name from online user, fall back to server-provided name,
                    // then stored name from the hash resolver, and finally a
                    // deterministic human-readable name generated from the hash.
                    let online_name = online_name_by_hash
                        .get(cert_hash.as_str())
                        .map(|n| (*n).to_owned());
                    let name = online_name.unwrap_or_else(|| resolve_entry_name(
                        &cert_hash,
                        entry.name.as_deref().unwrap_or_default(),
                        state.pchat_ctx.hash_name_resolver.as_deref(),
                    ));
                    let is_online = online_name_by_hash.contains_key(cert_hash.as_str());
                    crate::state::types::KeyHolderEntry {
                        cert_hash,
                        name,
                        is_online,
                    }
                })
                .collect();

            let _ = state.pchat_ctx.key_holders.insert(channel_id, holders.clone());

            // Sync server-provided holder list into key_manager so that
            // consent checks can skip peers who already hold the key.
            let holder_hashes: std::collections::HashSet<&str> = holders
                .iter()
                .filter(|e| !e.cert_hash.is_empty())
                .map(|e| e.cert_hash.as_str())
                .collect();

            if let Some(ref mut pchat) = state.pchat_ctx.pchat {
                pchat.key_manager.replace_key_holders(
                    channel_id,
                    holder_hashes.iter().map(|h| (*h).to_owned()).collect(),
                );
            }

            // Remove any pending consent prompts for peers the server now
            // confirms as holders -- they already have the key.
            let before_len = state.pchat_ctx.pending_key_shares.len();
            state.pchat_ctx.pending_key_shares.retain(|p| {
                !(p.channel_id == channel_id && holder_hashes.contains(p.peer_cert_hash.as_str()))
            });

            // Collect payload for deferred emit outside the lock.
            let share_requests_payload = if state.pchat_ctx.pending_key_shares.len() != before_len {
                state.conn.tauri_app_handle.as_ref().map(|app| {
                    let remaining: Vec<_> = state
                        .pchat_ctx.pending_key_shares
                        .iter()
                        .filter(|p| p.channel_id == channel_id)
                        .cloned()
                        .collect();
                    (
                        app.clone(),
                        crate::state::types::KeyShareRequestsChangedPayload {
                            channel_id,
                            pending: remaining,
                        },
                    )
                })
            } else {
                None
            };

            (holders, share_requests_payload)
        };

        // Emit outside the lock to avoid deadlock with Tauri IPC.
        if let Some((app, payload)) = share_requests_payload {
            use tauri::Emitter;
            let _ = app.emit("pchat-key-share-requests-changed", payload);
        }

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
        ctx.emit("new-message", NewMessagePayload { channel_id, sender_session: None });
    }
}

impl HandleMessage for mumble_tcp::PchatOfflineQueueDrain {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatOfflineQueueDrain");
        let channel_id = self.channel_id.unwrap_or(0);
        pchat::handle_proto_offline_queue_drain(&ctx.shared, self);
        ctx.emit("new-message", NewMessagePayload { channel_id, sender_session: None });
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatSenderKeyDistribution {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatSenderKeyDistribution");
        let sender_hash = self.sender_hash.clone().unwrap_or_default();
        let channel_id = self.channel_id.unwrap_or(0);
        let data = self.distribution.clone().unwrap_or_default();

        if pchat::handle_signal_sender_key_by_hash(&ctx.shared, &sender_hash, channel_id, &data) {
            ctx.emit_empty("state-changed");
        }
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

        // Persist the reaction in the local cache (Signal V1 channels).
        if let Ok(mut state) = ctx.shared.lock() {
            // Resolve the display name from the online user list so the
            // cache stores a human-readable name (the server may send the
            // cert hash instead of the display name).
            let resolved_name = state
                .users
                .values()
                .find(|u| u.hash.as_deref() == Some(&sender_hash))
                .map(|u| u.name.clone())
                .unwrap_or_else(|| sender_name.clone());

            if let Some(ref mut pchat_state) = state.pchat_ctx.pchat {
                if let Some(ref mut cache) = pchat_state.local_cache {
                    upsert_cached_reaction(
                        cache,
                        channel_id,
                        action_str,
                        CachedReaction {
                            message_id: message_id.clone(),
                            emoji: emoji.clone(),
                            sender_hash: sender_hash.clone(),
                            sender_name: resolved_name,
                            timestamp,
                        },
                    );
                }
            }
        }

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

        // Persist fetched reactions in the local cache (Signal V1 channels).
        if let Ok(mut state) = ctx.shared.lock() {
            // Build a hash -> name lookup before borrowing pchat mutably.
            let name_by_hash: HashMap<String, String> = state
                .users
                .values()
                .filter_map(|u| u.hash.clone().map(|h| (h, u.name.clone())))
                .collect();

            if let Some(ref mut pchat_state) = state.pchat_ctx.pchat {
                if let Some(ref mut cache) = pchat_state.local_cache {
                    bulk_insert_cached_reactions(cache, channel_id, &reactions, &name_by_hash);
                }
            }
        }

        ctx.emit(
            "pchat-reaction-fetch-response",
            ReactionFetchResponsePayload {
                channel_id,
                reactions,
            },
        );
    }
}

pub(super) fn resolve_entry_name(
    cert_hash: &str,
    server_name: &str,
    resolver: Option<&dyn crate::state::hash_names::HashNameResolver>,
) -> String {
    if !server_name.is_empty() && server_name != cert_hash {
        return server_name.to_owned();
    }
    resolver.map_or_else(|| cert_hash.to_owned(), |r| r.resolve(cert_hash))
}

impl HandleMessage for mumble_tcp::PchatPinDeliver {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatPinDeliver");
        let channel_id = self.channel_id.unwrap_or(0);
        let message_id = self.message_id.clone().unwrap_or_default();
        let pinned = !self.unpin.unwrap_or(false);
        let pinner_hash = self.pinner_hash.clone().unwrap_or_default();
        let pinner_name = self.pinner_name.clone().unwrap_or_default();
        let timestamp = self.timestamp.unwrap_or(0);

        if let Ok(mut state) = ctx.shared.lock() {
            let resolved_name = resolve_name_by_hash(&state, &pinner_hash, &pinner_name);
            apply_pin_to_message(&mut state, channel_id, &message_id, pinned, &resolved_name, timestamp);
        }

        ctx.emit(
            "pchat-pin-deliver",
            PinDeliverPayload {
                channel_id,
                message_id,
                pinned,
                pinner_hash,
                pinner_name,
                timestamp,
            },
        );
    }
}

impl HandleMessage for mumble_tcp::PchatPinFetchResponse {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatPinFetchResponse");
        let channel_id = self.channel_id.unwrap_or(0);

        let pins: Vec<StoredPinPayload> = self
            .pins
            .iter()
            .map(|p| StoredPinPayload {
                message_id: p.message_id.clone().unwrap_or_default(),
                pinner_hash: p.pinner_hash.clone().unwrap_or_default(),
                pinner_name: p.pinner_name.clone().unwrap_or_default(),
                timestamp: p.timestamp.unwrap_or(0),
            })
            .collect();

        if let Ok(mut state) = ctx.shared.lock() {
            for pin in &pins {
                apply_pin_to_message(&mut state, channel_id, &pin.message_id, true, &pin.pinner_name, pin.timestamp);
            }
        }

        ctx.emit(
            "pchat-pin-fetch-response",
            PinFetchResponsePayload {
                channel_id,
                pins,
            },
        );
    }
}

fn resolve_name_by_hash(state: &crate::state::SharedState, hash: &str, fallback: &str) -> String {
    state
        .users
        .values()
        .find(|u| u.hash.as_deref() == Some(hash))
        .map(|u| u.name.clone())
        .unwrap_or_else(|| fallback.to_owned())
}

fn apply_pin_to_message(
    state: &mut crate::state::SharedState,
    channel_id: u32,
    message_id: &str,
    pinned: bool,
    pinner_name: &str,
    timestamp: u64,
) {
    let Some(msgs) = state.msgs.by_channel.get_mut(&channel_id) else { return };
    let Some(msg) = msgs.iter_mut().find(|m: &&mut ChatMessage| m.message_id.as_deref() == Some(message_id)) else { return };
    msg.pinned = pinned;
    msg.pinned_by = if pinned { Some(pinner_name.to_owned()) } else { None };
    msg.pinned_at = if pinned { Some(timestamp) } else { None };
}

fn upsert_cached_reaction(
    cache: &mut LocalMessageCache,
    channel_id: u32,
    action_str: &str,
    reaction: CachedReaction,
) {
    if action_str == "add" {
        cache.insert_reaction(channel_id, reaction);
    } else {
        cache.remove_reaction(
            channel_id,
            &reaction.message_id,
            &reaction.emoji,
            &reaction.sender_hash,
        );
    }
    if let Err(e) = cache.save_reactions() {
        warn!("failed to save reaction cache: {e}");
    }
}

fn bulk_insert_cached_reactions(
    cache: &mut LocalMessageCache,
    channel_id: u32,
    reactions: &[StoredReactionPayload],
    name_by_hash: &HashMap<String, String>,
) {
    for r in reactions {
        let resolved = name_by_hash
            .get(&r.sender_hash)
            .cloned()
            .unwrap_or_else(|| r.sender_name.clone());
        cache.insert_reaction(
            channel_id,
            CachedReaction {
                message_id: r.message_id.clone(),
                emoji: r.emoji.clone(),
                sender_hash: r.sender_hash.clone(),
                sender_name: resolved,
                timestamp: r.timestamp,
            },
        );
    }
    if let Err(e) = cache.save_reactions() {
        warn!("failed to save reaction cache: {e}");
    }
}
