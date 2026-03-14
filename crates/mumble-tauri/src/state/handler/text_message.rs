use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::*;

impl HandleMessage for mumble_tcp::TextMessage {
    fn handle(&self, ctx: &HandlerContext) {
        let mut unreads_changed = false;
        let mut dm_unreads_changed = false;
        let mut group_unreads_changed = false;
        let mut dm_sender: Option<u32> = None;
        let mut group_msg_id: Option<String> = None;

        // A message is a DM when it targets specific sessions and has
        // no channel_id.  The Mumble server sets `tm.session` to the
        // list of targeted user sessions (for the recipient) and
        // `tm.channel_id` is empty.
        let is_dm = !self.session.is_empty() && self.channel_id.is_empty();

        // Check for a group chat marker before treating as a plain DM.
        // Format: <!-- FANCY_GROUP:uuid -->body
        let group_marker = if is_dm {
            const PREFIX: &str = "<!-- FANCY_GROUP:";
            const SUFFIX: &str = " -->";
            let msg = &self.message;
            if let Some(rest) = msg.strip_prefix(PREFIX) {
                rest.find(SUFFIX).map(|end| {
                    let gid = rest[..end].to_string();
                    let body_start = PREFIX.len() + end + SUFFIX.len();
                    let body = msg[body_start..].to_string();
                    (gid, body)
                })
            } else {
                None
            }
        } else {
            None
        };

        if let Ok(mut state) = ctx.shared.lock() {
            let actor = self.actor;
            let own_session = state.own_session;

            // Don't duplicate messages we sent ourselves.
            let is_own = actor == own_session && actor.is_some();
            if is_own {
                return;
            }

            let sender_name = actor
                .and_then(|sid| state.users.get(&sid))
                .map(|u| u.name.clone())
                .unwrap_or_else(|| "Server".into());

            if let Some((ref gid, ref stripped_body)) = group_marker {
                // Group message - route to the correct group conversation.
                if state.group_chats.contains_key(gid) {
                    let mut msg = ChatMessage {
                        sender_session: actor,
                        sender_name,
                        body: stripped_body.clone(),
                        channel_id: 0,
                        is_own: false,
                        dm_session: None,
                        group_id: Some(gid.clone()),
                        message_id: self.message_id.clone(),
                        timestamp: self.timestamp,
                    };
                    msg.ensure_id();
                    state
                        .group_messages
                        .entry(gid.clone())
                        .or_default()
                        .push(msg);

                    if state.selected_group.as_deref() != Some(gid) {
                        *state.group_unread_counts.entry(gid.clone()).or_insert(0) += 1;
                        group_unreads_changed = true;
                    }

                    group_msg_id = Some(gid.clone());
                }
            } else if is_dm {
                let body = self.message.clone();
                // Direct message - store keyed by the sender's session.
                if let Some(sender_session) = actor {
                    let mut msg = ChatMessage {
                        sender_session: actor,
                        sender_name,
                        body,
                        channel_id: 0,
                        is_own: false,
                        dm_session: Some(sender_session),
                        group_id: None,
                        message_id: self.message_id.clone(),
                        timestamp: self.timestamp,
                    };
                    msg.ensure_id();
                    state
                        .dm_messages
                        .entry(sender_session)
                        .or_default()
                        .push(msg);

                    // Increment DM unread if this DM conversation is not viewed.
                    if state.selected_dm_user != Some(sender_session) {
                        *state.dm_unread_counts.entry(sender_session).or_insert(0) += 1;
                        dm_unreads_changed = true;
                    }

                    dm_sender = Some(sender_session);
                }
            } else {
                // Channel message - original behaviour.
                let body = self.message.clone();
                let target_channels: Vec<u32> = if self.channel_id.is_empty() {
                    vec![0]
                } else {
                    self.channel_id.clone()
                };

                let selected = state.selected_channel;

                for &ch_id in &target_channels {
                    let mut msg = ChatMessage {
                        sender_session: actor,
                        sender_name: sender_name.clone(),
                        body: body.clone(),
                        channel_id: ch_id,
                        is_own: false,
                        dm_session: None,
                        group_id: None,
                        message_id: self.message_id.clone(),
                        timestamp: self.timestamp,
                    };
                    msg.ensure_id();
                    state.messages.entry(ch_id).or_default().push(msg);

                    // Increment unread count if this channel is not currently viewed.
                    if selected != Some(ch_id) {
                        *state.unread_counts.entry(ch_id).or_insert(0) += 1;
                        unreads_changed = true;
                    }

                    ctx.emit("new-message", NewMessagePayload { channel_id: ch_id });

                    // Flash the taskbar on Windows when a permanently-listened
                    // channel gets a message while it is not the viewed channel.
                    if state.permanently_listened.contains(&ch_id) && selected != Some(ch_id) {
                        ctx.request_user_attention();
                    }
                }
            }
        }

        // Emit group message events outside the lock.
        if let Some(gid) = group_msg_id {
            ctx.emit(
                "new-group-message",
                NewGroupMessagePayload { group_id: gid },
            );

            // Flash taskbar for group messages.
            ctx.request_user_attention();
        }

        if group_unreads_changed {
            let unreads = ctx
                .shared
                .lock()
                .map(|s| s.group_unread_counts.clone())
                .unwrap_or_default();
            ctx.emit("group-unread-changed", GroupUnreadPayload { unreads });
        }

        // Emit DM events outside the lock.
        if let Some(sender_session) = dm_sender {
            ctx.emit("new-dm", NewDmPayload { session: sender_session });

            // Always flash taskbar for DMs.
            ctx.request_user_attention();
        }

        if dm_unreads_changed {
            let unreads = ctx
                .shared
                .lock()
                .map(|s| s.dm_unread_counts.clone())
                .unwrap_or_default();
            ctx.emit("dm-unread-changed", DmUnreadPayload { unreads });
        }

        if unreads_changed {
            let unreads = ctx
                .shared
                .lock()
                .map(|s| s.unread_counts.clone())
                .unwrap_or_default();
            ctx.emit("unread-changed", UnreadPayload { unreads });
        }
    }
}
