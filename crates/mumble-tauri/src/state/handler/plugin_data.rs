use mumble_protocol::proto::mumble_tcp;
use tracing::debug;

use super::{HandleMessage, HandlerContext};
use crate::state::types::PluginDataPayload;

impl HandleMessage for mumble_tcp::PluginDataTransmission {
    fn handle(&self, ctx: &HandlerContext) {
        debug!(
            sender = ?self.sender_session,
            data_id = ?self.data_id,
            data_len = self.data.as_ref().map(Vec::len).unwrap_or(0),
            "plugin data received"
        );

        ctx.emit(
            "plugin-data",
            PluginDataPayload {
                sender_session: self.sender_session,
                data: self.data.clone().unwrap_or_default(),
                data_id: self.data_id.clone().unwrap_or_default(),
            },
        );
    }
}
