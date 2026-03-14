use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::LatencyPayload;

impl HandleMessage for mumble_tcp::Ping {
    fn handle(&self, ctx: &HandlerContext) {
        if let Some(ts) = self.timestamp {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let rtt_ms = now.saturating_sub(ts) as f64;
            ctx.emit("ping-latency", LatencyPayload { rtt_ms });
        }
    }
}
