use mumble_protocol::proto::mumble_tcp;
use tracing::info;

use super::{HandleMessage, HandlerContext};
use crate::state::types::{GroupChat, GroupCreatedPayload, PluginDataPayload};

impl HandleMessage for mumble_tcp::PluginDataTransmission {
    fn handle(&self, ctx: &HandlerContext) {
        info!(
            sender = ?self.sender_session,
            data_id = ?self.data_id,
            data_len = self.data.as_ref().map(Vec::len).unwrap_or(0),
            "plugin data received"
        );

        // Handle group chat creation/updates server-side so the
        // group is known before any group messages arrive.
        if self.data_id.as_deref() == Some("fancy-group") {
            if let Some(data) = &self.data {
                if let Ok(val) = serde_json::from_slice::<serde_json::Value>(data) {
                    if val.get("action").and_then(|a| a.as_str()) == Some("create") {
                        if let Some(group_val) = val.get("group") {
                            if let Ok(group) =
                                serde_json::from_value::<GroupChat>(group_val.clone())
                            {
                                info!(group_id = %group.id, name = %group.name, "group chat created via plugin data");
                                if let Ok(mut state) = ctx.shared.lock() {
                                    state
                                        .group_chats
                                        .insert(group.id.clone(), group.clone());
                                }
                                ctx.emit(
                                    "group-created",
                                    GroupCreatedPayload { group },
                                );
                            }
                        }
                    }
                }
            }
        }

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
