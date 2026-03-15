use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::{CurrentChannelPayload, UserEntry};

impl HandleMessage for mumble_tcp::UserState {
    fn handle(&self, ctx: &HandlerContext) {
        let Some(session) = self.session else { return };

        let (is_synced, own_channel_changed) = {
            let mut state_guard = ctx.shared.lock().ok();
            if let Some(ref mut state) = state_guard {
                let user = state.users.entry(session).or_insert_with(|| UserEntry {
                    session,
                    name: String::new(),
                    channel_id: 0,
                    texture: None,
                    comment: None,
                    mute: false,
                    deaf: false,
                    suppress: false,
                    self_mute: false,
                    self_deaf: false,
                    priority_speaker: false,
                });
                if let Some(ref name) = self.name {
                    user.name = name.clone();
                }
                if let Some(ref texture) = self.texture {
                    user.texture = if texture.is_empty() {
                        None
                    } else {
                        Some(texture.clone())
                    };
                }
                if let Some(ref comment) = self.comment {
                    user.comment = if comment.is_empty() {
                        None
                    } else {
                        Some(comment.clone())
                    };
                }
                if let Some(mute) = self.mute {
                    user.mute = mute;
                }
                if let Some(deaf) = self.deaf {
                    user.deaf = deaf;
                }
                if let Some(suppress) = self.suppress {
                    user.suppress = suppress;
                }
                if let Some(self_mute) = self.self_mute {
                    user.self_mute = self_mute;
                }
                if let Some(self_deaf) = self.self_deaf {
                    user.self_deaf = self_deaf;
                }
                if let Some(priority) = self.priority_speaker {
                    user.priority_speaker = priority;
                }
                let mut own_ch = false;
                if let Some(ch) = self.channel_id {
                    user.channel_id = ch;
                    // Track when our own user moves channels.
                    if state.own_session == Some(session) {
                        state.current_channel = Some(ch);
                        own_ch = true;
                    }
                }
                (state.synced, own_ch)
            } else {
                (false, false)
            }
        };
        // Notify frontend about current-channel change.
        if own_channel_changed {
            if let Some(ch) = self.channel_id {
                ctx.emit(
                    "current-channel-changed",
                    CurrentChannelPayload { channel_id: ch },
                );
            }
        }
        // Only notify frontend after initial sync is done.
        if is_synced {
            ctx.emit_empty("state-changed");
        }
    }
}
