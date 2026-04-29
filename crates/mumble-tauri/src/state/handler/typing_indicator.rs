use mumble_protocol::proto::mumble_tcp;
use serde::Serialize;
use tracing::debug;

use super::{HandleMessage, HandlerContext};

#[derive(Serialize, Clone)]
struct TypingIndicatorPayload {
    session: u32,
    channel_id: u32,
}

impl HandleMessage for mumble_tcp::FancyTypingIndicator {
    fn handle(&self, ctx: &HandlerContext) {
        let session = self.actor.unwrap_or(0);
        let channel_id = self.channel_id.unwrap_or(0);

        debug!(
            session,
            channel_id,
            raw_actor = ?self.actor,
            raw_channel_id = ?self.channel_id,
            "typing indicator handler invoked"
        );

        if session == 0 {
            debug!("typing indicator dropped: actor is 0/None");
            return;
        }

        debug!(session, channel_id, "received typing indicator");

        ctx.emit(
            "typing-indicator",
            TypingIndicatorPayload {
                session,
                channel_id,
            },
        );
    }
}
