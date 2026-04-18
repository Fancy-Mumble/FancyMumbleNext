use mumble_protocol::proto::mumble_tcp;
use serde::Serialize;
use tracing::debug;

use super::pchat::resolve_entry_name;
use super::{HandleMessage, HandlerContext};

#[derive(Serialize, Clone)]
struct ReadStatePayload {
    cert_hash: String,
    name: String,
    is_online: bool,
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

        let read_states = {
            let Ok(state) = ctx.shared.lock() else {
                return;
            };

            let online_hashes: std::collections::HashSet<&str> = state
                .users
                .values()
                .filter_map(|u| u.hash.as_deref())
                .collect();

            self.read_states
                .iter()
                .filter_map(|rs| {
                    let cert_hash = rs.cert_hash.clone().unwrap_or_default();
                    if cert_hash.is_empty() {
                        return None;
                    }
                    let server_name = rs.name.clone().unwrap_or_default();

                    let online_name = state
                        .users
                        .values()
                        .find(|u| u.hash.as_deref() == Some(cert_hash.as_str()))
                        .map(|u| u.name.clone());

                    let name = online_name.unwrap_or_else(|| {
                        resolve_entry_name(
                            &cert_hash,
                            &server_name,
                            state.pchat_ctx.hash_name_resolver.as_deref(),
                        )
                    });

                    let is_online = online_hashes.contains(cert_hash.as_str());

                    Some(ReadStatePayload {
                        cert_hash,
                        name,
                        is_online,
                        last_read_message_id: rs
                            .last_read_message_id
                            .clone()
                            .unwrap_or_default(),
                        timestamp: rs.timestamp.unwrap_or(0),
                    })
                })
                .collect::<Vec<_>>()
        };

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
