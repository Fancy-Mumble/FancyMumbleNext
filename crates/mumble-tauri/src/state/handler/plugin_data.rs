use mumble_protocol::persistent::DATA_ID_SIGNAL_SENDER_KEY;
use mumble_protocol::proto::mumble_tcp;
use tracing::info;

use super::{HandleMessage, HandlerContext};
use crate::state::pchat;
use crate::state::types::{GroupChat, GroupCreatedPayload, PluginDataPayload};

// -- Per-data_id handlers ------------------------------------------

fn handle_fancy_group(data: &[u8], ctx: &HandlerContext) {
    if let Some("create") = parse_action(data).as_deref() {
        handle_group_create(data, ctx);
    }
}

fn parse_action(data: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(data)
        .ok()?
        .get("action")?
        .as_str()
        .map(String::from)
}

fn handle_group_create(data: &[u8], ctx: &HandlerContext) {
    let val: serde_json::Value = match serde_json::from_slice(data) {
        Ok(v) => v,
        Err(_) => return,
    };

    let Some(group_val) = val.get("group") else {
        return;
    };

    let group: GroupChat = match serde_json::from_value(group_val.clone()) {
        Ok(g) => g,
        Err(_) => return,
    };

    info!(group_id = %group.id, name = %group.name, "group chat created via plugin data");

    if let Ok(mut state) = ctx.shared.lock() {
        let _ = state.group_chats.insert(group.id.clone(), group.clone());
    }

    ctx.emit("group-created", GroupCreatedPayload { group });
}

// -- Trait implementation ------------------------------------------

impl HandleMessage for mumble_tcp::PluginDataTransmission {
    fn handle(&self, ctx: &HandlerContext) {
        info!(
            sender = ?self.sender_session,
            data_id = ?self.data_id,
            data_len = self.data.as_ref().map(Vec::len).unwrap_or(0),
            "plugin data received"
        );

        if let Some(data) = &self.data {
            match self.data_id.as_deref() {
                Some("fancy-group") => handle_fancy_group(data, ctx),
                Some(DATA_ID_SIGNAL_SENDER_KEY) => {
                    let sender = self.sender_session.unwrap_or(0);
                    pchat::handle_signal_sender_key(&ctx.shared, sender, data);
                }
                _ => {}
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
