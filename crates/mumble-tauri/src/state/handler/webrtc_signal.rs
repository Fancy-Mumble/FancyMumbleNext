use mumble_protocol::proto::mumble_tcp;
use tracing::debug;

use super::{HandleMessage, HandlerContext};
use crate::state::types::WebRtcSignalPayload;

impl HandleMessage for mumble_tcp::WebRtcSignal {
    fn handle(&self, ctx: &HandlerContext) {
        debug!(
            sender = ?self.sender_session,
            target = ?self.target_session,
            signal_type = ?self.signal_type,
            "webrtc signal received"
        );

        ctx.emit(
            "webrtc-signal",
            WebRtcSignalPayload {
                sender_session: self.sender_session,
                target_session: self.target_session,
                signal_type: self.signal_type.unwrap_or(0),
                payload: self.payload.clone().unwrap_or_default(),
            },
        );
    }
}
