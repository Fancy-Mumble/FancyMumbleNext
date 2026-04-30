use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::{ConnectionStatus, RejectedPayload};

impl HandleMessage for mumble_tcp::Reject {
    fn handle(&self, ctx: &HandlerContext) {
        let reason = self
            .reason
            .clone()
            .unwrap_or_else(|| "Connection rejected by server".into());
        if let Ok(mut state) = ctx.shared.lock() {
            state.conn.status = ConnectionStatus::Disconnected;
            state.conn.client_handle = None;
            state.conn.event_loop_handle = None;
        }
        ctx.emit(
            "connection-rejected",
            RejectedPayload {
                reason,
                reject_type: self.r#type,
            },
        );
    }
}
