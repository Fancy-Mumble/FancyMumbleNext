use mumble_protocol::proto::mumble_tcp;
use mumble_protocol::state::PchatMode;

use super::{HandleMessage, HandlerContext};
use crate::state::types::*;
use crate::state::SharedState;

// -- Message classification ----------------------------------------

/// Parsed group marker extracted from `<!-- FANCY_GROUP:uuid -->body`.
struct GroupMarker {
    group_id: String,
    body: String,
}

/// Classification of an incoming `TextMessage`.
enum MessageKind {
    Group(GroupMarker),
    DirectMessage,
    Channel,
}

fn parse_group_marker(message: &str) -> Option<GroupMarker> {
    const PREFIX: &str = "<!-- FANCY_GROUP:";
    const SUFFIX: &str = " -->";

    let rest = message.strip_prefix(PREFIX)?;
    let end = rest.find(SUFFIX)?;
    let group_id = rest[..end].to_string();
    let body_start = PREFIX.len() + end + SUFFIX.len();
    let body = message[body_start..].to_string();
    Some(GroupMarker { group_id, body })
}

fn classify(tm: &mumble_tcp::TextMessage) -> MessageKind {
    let is_dm = !tm.session.is_empty() && tm.channel_id.is_empty();
    if is_dm {
        match parse_group_marker(&tm.message) {
            Some(marker) => MessageKind::Group(marker),
            None => MessageKind::DirectMessage,
        }
    } else {
        MessageKind::Channel
    }
}

// -- Deferred events emitted after releasing the lock --------------

enum DeferredEvent {
    GroupMessage { group_id: String },
    GroupUnreads,
    DirectMessage { sender_session: u32 },
    DmUnreads,
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
                DeferredEvent::GroupMessage { group_id } => self.emit_group_message(group_id),
                DeferredEvent::GroupUnreads => self.emit_group_unreads(),
                DeferredEvent::DirectMessage { sender_session } => {
                    self.emit_direct_message(*sender_session);
                }
                DeferredEvent::DmUnreads => self.emit_dm_unreads(),
                DeferredEvent::ChannelUnreads => self.emit_channel_unreads(),
            }
        }
    }

    fn emit_group_message(&self, group_id: &str) {
        self.ctx.emit(
            "new-group-message",
            NewGroupMessagePayload {
                group_id: group_id.to_owned(),
            },
        );
        self.ctx.request_user_attention();
    }

    fn emit_group_unreads(&self) {
        let unreads = self
            .ctx
            .shared
            .lock()
            .map(|s| s.group_unread_counts.clone())
            .unwrap_or_default();
        self.ctx
            .emit("group-unread-changed", GroupUnreadPayload { unreads });
    }

    fn emit_direct_message(&self, sender_session: u32) {
        self.ctx.emit(
            "new-dm",
            NewDmPayload {
                session: sender_session,
            },
        );
        self.ctx.request_user_attention();
    }

    fn emit_dm_unreads(&self) {
        let unreads = self
            .ctx
            .shared
            .lock()
            .map(|s| s.dm_unread_counts.clone())
            .unwrap_or_default();
        self.ctx
            .emit("dm-unread-changed", DmUnreadPayload { unreads });
    }

    fn emit_channel_unreads(&self) {
        let unreads = self
            .ctx
            .shared
            .lock()
            .map(|s| s.unread_counts.clone())
            .unwrap_or_default();
        self.ctx
            .emit("unread-changed", UnreadPayload { unreads });
    }
}

// -- Per-kind handlers ---------------------------------------------

fn resolve_sender_name(state: &SharedState, actor: Option<u32>) -> String {
    actor
        .and_then(|sid| state.users.get(&sid))
        .map(|u| u.name.clone())
        .unwrap_or_else(|| "Server".into())
}

fn handle_group_message(
    marker: &GroupMarker,
    tm: &mumble_tcp::TextMessage,
    state: &mut SharedState,
    deferred: &mut DeferredEmitter,
) {
    if !state.group_chats.contains_key(&marker.group_id) {
        return;
    }

    let sender_name = resolve_sender_name(state, tm.actor);
    let mut msg = ChatMessage {
        sender_session: tm.actor,
        sender_name,
        body: marker.body.clone(),
        channel_id: 0,
        is_own: false,
        dm_session: None,
        group_id: Some(marker.group_id.clone()),
        message_id: tm.message_id.clone(),
        timestamp: tm.timestamp,
        is_legacy: false,
    };
    msg.ensure_id();
    state
        .group_messages
        .entry(marker.group_id.clone())
        .or_default()
        .push(msg);

    if state.selected_group.as_deref() != Some(&marker.group_id) {
        *state
            .group_unread_counts
            .entry(marker.group_id.clone())
            .or_insert(0) += 1;
        deferred.push(DeferredEvent::GroupUnreads);
    }

    deferred.push(DeferredEvent::GroupMessage {
        group_id: marker.group_id.clone(),
    });
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
        body: tm.message.clone(),
        channel_id: 0,
        is_own: false,
        dm_session: Some(sender_session),
        group_id: None,
        message_id: tm.message_id.clone(),
        timestamp: tm.timestamp,
        is_legacy: false,
    };
    msg.ensure_id();
    state
        .dm_messages
        .entry(sender_session)
        .or_default()
        .push(msg);

    if state.selected_dm_user != Some(sender_session) {
        *state
            .dm_unread_counts
            .entry(sender_session)
            .or_insert(0) += 1;
        deferred.push(DeferredEvent::DmUnreads);
    }

    deferred.push(DeferredEvent::DirectMessage { sender_session });
}

fn handle_channel_message(
    tm: &mumble_tcp::TextMessage,
    state: &mut SharedState,
    ctx: &HandlerContext,
    deferred: &mut DeferredEmitter,
) {
    let target_channels: Vec<u32> = if tm.channel_id.is_empty() {
        vec![0]
    } else {
        tm.channel_id.clone()
    };

    let selected = state.selected_channel;
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
            .and_then(|c| c.pchat_mode)
            .is_some_and(|m| !matches!(m, PchatMode::None));

        let sender_has_e2ee = tm
            .actor
            .and_then(|sid| state.users.get(&sid))
            .is_some_and(|u| u.has_pchat_e2ee());

        if has_pchat && sender_has_e2ee {
            // Fancy sender — PchatMessageDeliver is the authoritative source.
            continue;
        }

        let is_legacy = has_pchat && !sender_has_e2ee;

        let mut msg = ChatMessage {
            sender_session: tm.actor,
            sender_name: sender_name.clone(),
            body: tm.message.clone(),
            channel_id: ch_id,
            is_own: false,
            dm_session: None,
            group_id: None,
            message_id: tm.message_id.clone(),
            timestamp: tm.timestamp,
            is_legacy,
        };
        msg.ensure_id();
        state.messages.entry(ch_id).or_default().push(msg);

        if selected != Some(ch_id) {
            *state.unread_counts.entry(ch_id).or_insert(0) += 1;
            unreads_changed = true;
        }

        ctx.emit("new-message", NewMessagePayload { channel_id: ch_id });

        // Flash the taskbar when a permanently-listened channel gets a
        // message while it is not the viewed channel.
        if state.permanently_listened.contains(&ch_id) && selected != Some(ch_id) {
            ctx.request_user_attention();
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
            // Don't duplicate messages we sent ourselves.
            if self.actor == state.own_session && self.actor.is_some() {
                return;
            }

            match &kind {
                MessageKind::Group(marker) => {
                    handle_group_message(marker, self, &mut state, &mut deferred);
                }
                MessageKind::DirectMessage => {
                    handle_direct_message(self, &mut state, &mut deferred);
                }
                MessageKind::Channel => {
                    handle_channel_message(self, &mut state, ctx, &mut deferred);
                }
            }
        }

        deferred.flush();
    }
}
