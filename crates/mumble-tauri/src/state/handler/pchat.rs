use mumble_protocol::proto::mumble_tcp;
use tracing::debug;

use super::{HandleMessage, HandlerContext};
use crate::state::pchat;
use crate::state::types::{NewMessagePayload, PchatFetchCompletePayload, PchatHistoryLoadingPayload};

impl HandleMessage for mumble_tcp::PchatMessageDeliver {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatMessageDeliver");
        let channel_id = self.channel_id.unwrap_or(0);
        pchat::handle_proto_msg_deliver(&ctx.shared, self);
        ctx.emit("new-message", NewMessagePayload { channel_id });
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatFetchResponse {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatFetchResponse");
        let channel_id = self.channel_id.unwrap_or(0);
        let has_more = self.has_more.unwrap_or(false);
        let total_stored = self.total_stored.unwrap_or(0);
        pchat::handle_proto_fetch_resp(&ctx.shared, self);
        // Signal that history loading is complete for this channel.
        ctx.emit("pchat-history-loading", PchatHistoryLoadingPayload { channel_id, loading: false });
        ctx.emit("pchat-fetch-complete", PchatFetchCompletePayload { channel_id, has_more, total_stored });
        ctx.emit("new-message", NewMessagePayload { channel_id });
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatKeyAnnounce {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatKeyAnnounce");
        pchat::handle_proto_key_announce(&ctx.shared, self);
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatKeyExchange {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatKeyExchange");
        pchat::handle_proto_key_exchange(&ctx.shared, self);
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatKeyRequest {
    fn handle(&self, ctx: &HandlerContext) {
        debug!("received PchatKeyRequest");
        pchat::handle_proto_key_request(&ctx.shared, self);
        ctx.emit_empty("state-changed");
    }
}

impl HandleMessage for mumble_tcp::PchatAck {
    fn handle(&self, _ctx: &HandlerContext) {
        debug!("received PchatAck");
        pchat::handle_proto_ack(self);
    }
}
