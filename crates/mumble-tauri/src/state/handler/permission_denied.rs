use mumble_protocol::proto::mumble_tcp;
use tracing::info;

use super::{HandleMessage, HandlerContext};
use crate::state::types::{ListenDeniedPayload, PermissionDeniedPayload};

impl HandleMessage for mumble_tcp::PermissionDenied {
    fn handle(&self, ctx: &HandlerContext) {
        info!(
            reason = ?self.reason,
            r#type = ?self.r#type,
            channel_id = ?self.channel_id,
            "permission denied received"
        );

        if let Some(ch_id) = self.channel_id {
            if let Ok(mut state) = ctx.shared.lock() {
                if state.permanently_listened.remove(&ch_id) {
                    info!(ch_id, "reverted permanent listen due to permission denied");
                }
            }
            ctx.emit(
                "listen-denied",
                ListenDeniedPayload { channel_id: ch_id },
            );
        }

        // Always emit a general permission-denied event so the
        // frontend can surface errors (e.g. profile too large).
        ctx.emit(
            "permission-denied",
            PermissionDeniedPayload {
                deny_type: self.r#type,
                reason: self.reason.clone(),
            },
        );
    }
}
