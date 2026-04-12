use mumble_protocol::proto::mumble_tcp;
use serde::Serialize;
use tracing::debug;

use super::{HandleMessage, HandlerContext};

#[derive(Serialize, Clone)]
struct ReadStatePayload {
    cert_hash: String,
    name: String,
    last_read_message_id: String,
    timestamp: u64,
}

#[derive(Serialize, Clone)]
struct ReadReceiptDeliverPayload {
    channel_id: u32,
    read_states: Vec<ReadStatePayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query_message_id: Option<String>,
}

impl HandleMessage for mumble_tcp::FancyReadReceiptDeliver {
    fn handle(&self, ctx: &HandlerContext) {
        let channel_id = self.channel_id.unwrap_or(0);

        let read_states: Vec<ReadStatePayload> = self
            .read_states
            .iter()
            .map(|rs| ReadStatePayload {
                cert_hash: rs.cert_hash.clone().unwrap_or_default(),
                name: rs.name.clone().unwrap_or_default(),
                last_read_message_id: rs.last_read_message_id.clone().unwrap_or_default(),
                timestamp: rs.timestamp.unwrap_or(0),
            })
            .collect();

        debug!(
            channel_id,
            count = read_states.len(),
            query = self.query_message_id.is_some(),
            "received FancyReadReceiptDeliver"
        );

        let payload = ReadReceiptDeliverPayload {
            channel_id,
            read_states,
            query_message_id: self.query_message_id.clone(),
        };

        ctx.emit("read-receipt-deliver", payload);
    }
}
