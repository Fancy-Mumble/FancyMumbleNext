use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::{RegisteredUserPayload, UserCommentPayload};

impl HandleMessage for mumble_tcp::UserList {
    fn handle(&self, ctx: &HandlerContext) {
        // A blob response is a single-user message with the full comment set.
        // Emit it as a targeted "user-comment" event rather than replacing the
        // whole registered-user list in the UI.
        if self.users.len() == 1 {
            let u = &self.users[0];
            if let Some(comment) = u.comment.as_deref().filter(|c| !c.is_empty()) {
                ctx.emit("user-comment", UserCommentPayload {
                    user_id: u.user_id,
                    comment: comment.to_owned(),
                });
                return;
            }
        }

        let users: Vec<RegisteredUserPayload> = self
            .users
            .iter()
            .map(|u| RegisteredUserPayload {
                user_id: u.user_id,
                name: u.name.clone().unwrap_or_default(),
                last_seen: u.last_seen.clone(),
                last_channel: u.last_channel,
                texture: u.texture.as_ref().filter(|t| !t.is_empty()).cloned(),
                comment: u.comment.as_ref().filter(|c| !c.is_empty()).cloned(),
                comment_hash: u.comment_hash.as_ref().filter(|h| !h.is_empty()).cloned(),
            })
            .collect();
        ctx.emit("user-list", users);
    }
}
