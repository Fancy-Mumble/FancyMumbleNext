use fancy_utils::html::strip_html_tags;
use mumble_protocol::proto::mumble_tcp;
use mumble_protocol::persistent::PchatProtocol;

use super::{HandleMessage, HandlerContext};
use crate::state::types::*;
use crate::state::SharedState;

// -- Message classification ----------------------------------------

/// Classification of an incoming `TextMessage`.
enum MessageKind {
    DirectMessage,
    Channel,
}

fn classify(tm: &mumble_tcp::TextMessage) -> MessageKind {
    let is_dm = !tm.session.is_empty() && tm.channel_id.is_empty();
    if is_dm {
        MessageKind::DirectMessage
    } else {
        MessageKind::Channel
    }
}

// -- Deferred events emitted after releasing the lock --------------

enum DeferredEvent {
    DirectMessage {
        sender_session: u32,
        sender_name: String,
        body: String,
    },
    DmUnreads,
    NewMessage {
        channel_id: u32,
        sender_session: Option<u32>,
    },
    RequestUserAttention,
    ChannelMessage {
        channel_id: u32,
        sender_name: String,
        body: String,
        sender_session: Option<u32>,
    },
    ChannelUnreads,
}

struct DeferredEmitter<'a> {
    events: Vec<DeferredEvent>,
    ctx: &'a HandlerContext,
}

impl<'a> DeferredEmitter<'a> {
    fn new(ctx: &'a HandlerContext) -> Self {
        Self {
            events: Vec::new(),
            ctx,
        }
    }

    fn push(&mut self, event: DeferredEvent) {
        self.events.push(event);
    }

    fn flush(self) {
        for event in &self.events {
            match event {
                DeferredEvent::DirectMessage {
                    sender_session,
                    sender_name,
                    body,
                } => {
                    self.emit_direct_message(*sender_session, sender_name, body);
                }
                DeferredEvent::DmUnreads => self.emit_dm_unreads(),
                DeferredEvent::NewMessage { channel_id, sender_session } => {
                    self.ctx.emit("new-message", NewMessagePayload { channel_id: *channel_id, sender_session: *sender_session });
                }
                DeferredEvent::RequestUserAttention => {
                    self.ctx.request_user_attention();
                }
                DeferredEvent::ChannelMessage {
                    channel_id,
                    sender_name,
                    body,
                    sender_session,
                } => self.emit_channel_notification(*channel_id, sender_name, body, *sender_session),
                DeferredEvent::ChannelUnreads => self.emit_channel_unreads(),
            }
        }
    }

    fn emit_direct_message(&self, sender_session: u32, sender_name: &str, body: &str) {
        self.ctx.emit(
            "new-dm",
            NewDmPayload {
                session: sender_session,
            },
        );
        self.ctx.request_user_attention();
        let icon = self.lookup_texture(Some(sender_session));
        self.ctx
            .send_notification_with_icon(sender_name, &strip_html_tags(body), icon.as_deref(), None);
    }

    fn emit_dm_unreads(&self) {
        let unreads = self
            .ctx
            .shared
            .lock()
            .map(|s| s.msgs.dm_unread.clone())
            .unwrap_or_default();
        self.ctx
            .emit("dm-unread-changed", DmUnreadPayload { unreads });
    }

    fn emit_channel_unreads(&self) {
        let unreads = self
            .ctx
            .shared
            .lock()
            .map(|s| s.msgs.channel_unread.clone())
            .unwrap_or_default();
        self.ctx
            .emit("unread-changed", UnreadPayload { unreads });
    }

    fn emit_channel_notification(
        &self,
        channel_id: u32,
        sender_name: &str,
        body: &str,
        sender_session: Option<u32>,
    ) {
        let (channel_name, icon) = self
            .ctx
            .shared
            .lock()
            .ok()
            .map(|s| {
                let name = s.channels.get(&channel_id).map(|c| c.name.clone());
                let texture = sender_session
                    .and_then(|sid| s.users.get(&sid).and_then(|u| u.texture.clone()));
                (name, texture)
            })
            .unwrap_or_default();
        let title = match channel_name {
            Some(name) => format!("{sender_name} in #{name}"),
            None => sender_name.to_owned(),
        };
        self.ctx
            .send_notification_with_icon(&title, &strip_html_tags(body), icon.as_deref(), Some(channel_id));
    }

    fn lookup_texture(&self, session: Option<u32>) -> Option<Vec<u8>> {
        let sid = session?;
        self.ctx
            .shared
            .lock()
            .ok()?
            .users
            .get(&sid)?
            .texture
            .clone()
    }
}

// -- Helpers -------------------------------------------------------

/// Apply an edit to a message list, returning `true` if the target was found.
fn apply_edit(messages: &mut [ChatMessage], edit_id: &str, new_body: &str, edited_at: u64) -> bool {
    if let Some(msg) = messages.iter_mut().find(|m| m.message_id.as_deref() == Some(edit_id)) {
        msg.body = new_body.to_owned();
        msg.edited_at = Some(edited_at);
        true
    } else {
        false
    }
}

fn emit_edit_events(
    kind: &MessageKind,
    tm: &mumble_tcp::TextMessage,
    _state: &SharedState,
    ctx: &HandlerContext,
    deferred: &mut DeferredEmitter<'_>,
) {
    match kind {
        MessageKind::DirectMessage => {
            if let Some(sid) = tm.actor {
                ctx.emit("dm-edited", NewDmPayload { session: sid });
            }
        }
        MessageKind::Channel => {
            let ids = if tm.channel_id.is_empty() { &[0u32][..] } else { &tm.channel_id };
            for &ch_id in ids {
                deferred.push(DeferredEvent::NewMessage { channel_id: ch_id, sender_session: tm.actor });
            }
        }
    }
}

fn try_apply_edit(
    tm: &mumble_tcp::TextMessage,
    kind: &MessageKind,
    edit_id: &str,
    state: &mut SharedState,
) -> bool {
    let edited_at = tm.timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    });
    match kind {
        MessageKind::DirectMessage => {
            tm.actor
                .and_then(|sid| state.msgs.by_dm.get_mut(&sid))
                .is_some_and(|msgs| apply_edit(msgs, edit_id, &tm.message, edited_at))
        }
        MessageKind::Channel => {
            let channel_ids = if tm.channel_id.is_empty() { vec![0u32] } else { tm.channel_id.clone() };
            channel_ids.iter().any(|ch_id| {
                state.msgs.by_channel.get_mut(ch_id)
                    .is_some_and(|msgs| apply_edit(msgs, edit_id, &tm.message, edited_at))
            })
        }
    }
}

// -- Per-kind handlers ---------------------------------------------

fn resolve_sender_name(state: &SharedState, actor: Option<u32>) -> String {
    actor
        .and_then(|sid| state.users.get(&sid))
        .map(|u| u.name.clone())
        .unwrap_or_else(|| "Server".into())
}

fn resolve_sender_hash(state: &SharedState, actor: Option<u32>) -> Option<String> {
    actor
        .and_then(|sid| state.users.get(&sid))
        .and_then(|u| u.hash.clone())
}

fn handle_direct_message(
    tm: &mumble_tcp::TextMessage,
    state: &mut SharedState,
    deferred: &mut DeferredEmitter,
) {
    let Some(sender_session) = tm.actor else {
        return;
    };

    let sender_name = resolve_sender_name(state, tm.actor);
    let mut msg = ChatMessage {
        sender_session: tm.actor,
        sender_name,
        sender_hash: resolve_sender_hash(state, tm.actor),
        body: tm.message.clone(),
        channel_id: 0,
        is_own: false,
        dm_session: Some(sender_session),
        message_id: tm.message_id.clone(),
        timestamp: tm.timestamp,
        is_legacy: false,
        edited_at: None,
        pinned: false,
        pinned_by: None,
        pinned_at: None,
    };
    msg.ensure_id();
    state
        .msgs.by_dm
        .entry(sender_session)
        .or_default()
        .push(msg);

    if state.msgs.selected_dm_user != Some(sender_session) {
        *state
            .msgs.dm_unread
            .entry(sender_session)
            .or_insert(0) += 1;
        deferred.push(DeferredEvent::DmUnreads);
    }

    deferred.push(DeferredEvent::DirectMessage {
        sender_session,
        sender_name: resolve_sender_name(state, tm.actor),
        body: tm.message.clone(),
    });
}

fn handle_channel_message(
    tm: &mumble_tcp::TextMessage,
    state: &mut SharedState,
    deferred: &mut DeferredEmitter,
) {
    let target_channels: Vec<u32> = if tm.channel_id.is_empty() {
        vec![0]
    } else {
        tm.channel_id.clone()
    };

    let selected = state.selected_channel;
    let app_focused = state.prefs.app_focused;
    let sender_name = resolve_sender_name(state, tm.actor);
    let mut unreads_changed = false;

    for &ch_id in &target_channels {
        // For pchat-enabled channels, check whether the sender supports E2EE.
        // If they do, skip — the authoritative PchatMessageDeliver will arrive
        // separately.  If they don't (legacy client), accept the TextMessage
        // and mark it as legacy so the UI can style it differently.
        let has_pchat = state
            .channels
            .get(&ch_id)
            .and_then(|c| c.pchat_protocol)
            .is_some_and(|m| !matches!(m, PchatProtocol::None));

        let sender_has_e2ee = tm
            .actor
            .and_then(|sid| state.users.get(&sid))
            .is_some_and(UserEntry::has_pchat_e2ee);

        if has_pchat && sender_has_e2ee {
            // Fancy sender — PchatMessageDeliver is the authoritative source.
            continue;
        }

        let is_legacy = has_pchat && !sender_has_e2ee;

        let mut msg = ChatMessage {
            sender_session: tm.actor,
            sender_name: sender_name.clone(),
            sender_hash: resolve_sender_hash(state, tm.actor),
            body: tm.message.clone(),
            channel_id: ch_id,
            is_own: false,
            dm_session: None,
            message_id: tm.message_id.clone(),
            timestamp: tm.timestamp,
            is_legacy,
            edited_at: None,
            pinned: false,
            pinned_by: None,
            pinned_at: None,
        };
        msg.ensure_id();
        let bucket = state.msgs.by_channel.entry(ch_id).or_default();
        crate::state::push_capped(bucket, msg);

        if selected != Some(ch_id) {
            *state.msgs.channel_unread.entry(ch_id).or_insert(0) += 1;
            unreads_changed = true;
        }

        deferred.push(DeferredEvent::NewMessage { channel_id: ch_id, sender_session: tm.actor });

        // Flash the taskbar when a permanently-listened channel gets a
        // message while it is not the viewed channel.
        if state.permanently_listened.contains(&ch_id) && selected != Some(ch_id) {
            deferred.push(DeferredEvent::RequestUserAttention);
        }

        // Native notification for messages arriving in non-viewed channels,
        // or for ANY channel when the app is not focused (backgrounded).
        if selected != Some(ch_id) || !app_focused {
            deferred.push(DeferredEvent::ChannelMessage {
                channel_id: ch_id,
                sender_name: sender_name.clone(),
                body: tm.message.clone(),
                sender_session: tm.actor,
            });
        }
    }

    if unreads_changed {
        deferred.push(DeferredEvent::ChannelUnreads);
    }
}

// -- Trait implementation ------------------------------------------

impl HandleMessage for mumble_tcp::TextMessage {
    fn handle(&self, ctx: &HandlerContext) {
        let kind = classify(self);
        let mut deferred = DeferredEmitter::new(ctx);

        if let Ok(mut state) = ctx.shared.lock() {
            // Don't duplicate messages we sent ourselves (regular sends).
            // For edits from ourselves, we *do* need to process them because
            // the local edit_message path already applied the change locally,
            // and the server won't echo edits back to us.
            if self.actor == state.conn.own_session && self.actor.is_some() && self.edit_id.is_none() {
                return;
            }

            // Handle message edits: find and update the existing message.
            if let Some(ref edit_id) = self.edit_id {
                if try_apply_edit(self, &kind, edit_id, &mut state) {
                    emit_edit_events(&kind, self, &state, ctx, &mut deferred);
                }
                drop(state);
                deferred.flush();
                return;
            }

            match &kind {
                MessageKind::DirectMessage => {
                    handle_direct_message(self, &mut state, &mut deferred);
                }
                MessageKind::Channel => {
                    handle_channel_message(self, &mut state, &mut deferred);
                }
            }
        }

        deferred.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_message(id: &str, body: &str) -> ChatMessage {
        ChatMessage {
            sender_session: Some(1),
            sender_name: "Alice".into(),
            body: body.into(),
            channel_id: 0,
            is_own: false,
            is_legacy: false,
            message_id: Some(id.into()),
            timestamp: Some(1000),
            sender_hash: None,
            edited_at: None,
            dm_session: None,
            pinned: false,
            pinned_by: None,
            pinned_at: None,
        }
    }

    #[test]
    fn apply_edit_updates_existing_message() {
        let mut messages = vec![
            make_message("msg-1", "original body"),
            make_message("msg-2", "other message"),
        ];
        let result = apply_edit(&mut messages, "msg-1", "updated body", 2000);
        assert!(result);
        assert_eq!(messages[0].body, "updated body");
        assert_eq!(messages[0].edited_at, Some(2000));
        assert_eq!(messages[1].body, "other message");
        assert!(messages[1].edited_at.is_none());
    }

    #[test]
    fn apply_edit_returns_false_for_unknown_id() {
        let mut messages = vec![make_message("msg-1", "original body")];
        let result = apply_edit(&mut messages, "nonexistent", "new body", 2000);
        assert!(!result);
        assert_eq!(messages[0].body, "original body");
        assert!(messages[0].edited_at.is_none());
    }

    #[test]
    fn apply_edit_on_empty_list() {
        let mut messages: Vec<ChatMessage> = vec![];
        let result = apply_edit(&mut messages, "any-id", "body", 1000);
        assert!(!result);
    }
}
