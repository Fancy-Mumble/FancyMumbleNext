use tracing::info;
use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::pchat;

impl HandleMessage for mumble_tcp::ChannelRemove {
    fn handle(&self, ctx: &HandlerContext) {
        if let Ok(mut state) = ctx.shared.lock() {
            let _ = state.channels.remove(&self.channel_id);
            let _ = state.messages.remove(&self.channel_id);

            // Clear pchat key material so stale keys are not reused if the
            // server recycles this channel ID.
            if let Some(ref mut p) = state.pchat {
                p.key_manager.remove_channel(self.channel_id);
                let _ = p.fetched_channels.remove(&self.channel_id);
                info!(channel_id = self.channel_id, "cleared pchat state for removed channel");

                // Remove persisted archive key from disk.
                if let Some(ref dir) = p.identity_dir {
                    pchat::delete_persisted_archive_key(dir, self.channel_id);
                }
            }

            // Clear UI holder list.
            let _ = state.key_holders.remove(&self.channel_id);

            // Remove any pending key-share consent for this channel.
            state.pending_key_shares.retain(|p| p.channel_id != self.channel_id);
        }
        ctx.emit_empty("state-changed");
    }
}
