use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::UserStatsPayload;

impl HandleMessage for mumble_tcp::UserStats {
    fn handle(&self, ctx: &HandlerContext) {
        let Some(session) = self.session else { return };

        let payload = UserStatsPayload {
            session,
            tcp_packets: self.tcp_packets.unwrap_or(0),
            udp_packets: self.udp_packets.unwrap_or(0),
            tcp_ping_avg: self.tcp_ping_avg.unwrap_or(0.0),
            tcp_ping_var: self.tcp_ping_var.unwrap_or(0.0),
            udp_ping_avg: self.udp_ping_avg.unwrap_or(0.0),
            udp_ping_var: self.udp_ping_var.unwrap_or(0.0),
            bandwidth: self.bandwidth,
            onlinesecs: self.onlinesecs,
            idlesecs: self.idlesecs,
            strong_certificate: self.strong_certificate.unwrap_or(false),
            opus: self.opus.unwrap_or(false),
        };

        ctx.emit("user-stats", payload);
    }
}
