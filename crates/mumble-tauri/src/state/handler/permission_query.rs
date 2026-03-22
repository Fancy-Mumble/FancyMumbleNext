use mumble_protocol::proto::mumble_tcp;
use tracing::{debug, warn};

use super::{HandleMessage, HandlerContext};

impl HandleMessage for mumble_tcp::PermissionQuery {
    fn handle(&self, ctx: &HandlerContext) {
        debug!(
            channel_id = ?self.channel_id,
            permissions = ?self.permissions,
            flush = self.flush(),
            "permission query response received"
        );

        // If flush is set, clear all cached permissions first.
        if self.flush() {
            if let Ok(mut state) = ctx.shared.lock() {
                for ch in state.channels.values_mut() {
                    ch.permissions = None;
                }
            }
        }

        // Store the permission bitmask on the channel entry.
        if let (Some(channel_id), Some(perms)) = (self.channel_id, self.permissions) {
            if let Ok(mut state) = ctx.shared.lock() {
                if let Some(ch) = state.channels.get_mut(&channel_id) {
                    ch.permissions = Some(perms);
                } else {
                    warn!(channel_id, "permission query for unknown channel");
                }
            }
            // Notify the frontend that channel data changed.
            ctx.emit_empty("state-changed");
        } else if self.channel_id.is_some() && self.permissions.is_none() {
            warn!(
                channel_id = ?self.channel_id,
                "permission query response has no permissions field"
            );
        }
    }
}
