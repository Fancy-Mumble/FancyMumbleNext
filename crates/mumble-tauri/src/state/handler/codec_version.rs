use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};

impl HandleMessage for mumble_tcp::CodecVersion {
    fn handle(&self, ctx: &HandlerContext) {
        if let Ok(mut state) = ctx.shared.lock() {
            state.server.opus = self.opus();
        }
    }
}
