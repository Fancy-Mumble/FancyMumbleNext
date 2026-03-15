use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};

impl HandleMessage for mumble_tcp::ChannelRemove {
    fn handle(&self, ctx: &HandlerContext) {
        if let Ok(mut state) = ctx.shared.lock() {
            state.channels.remove(&self.channel_id);
            state.messages.remove(&self.channel_id);
        }
        ctx.emit_empty("state-changed");
    }
}
