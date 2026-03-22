use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::RegisteredUserPayload;

impl HandleMessage for mumble_tcp::UserList {
    fn handle(&self, ctx: &HandlerContext) {
        let users: Vec<RegisteredUserPayload> = self
            .users
            .iter()
            .map(|u| RegisteredUserPayload {
                user_id: u.user_id,
                name: u.name.clone().unwrap_or_default(),
                last_seen: u.last_seen.clone(),
                last_channel: u.last_channel,
            })
            .collect();
        ctx.emit("user-list", users);
    }
}
