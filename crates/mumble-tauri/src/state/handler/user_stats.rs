use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::{PacketStats, RollingStatsPayload, UserStatsPayload};

fn extract_packet_stats(
    stats: &Option<mumble_tcp::user_stats::Stats>,
) -> Option<PacketStats> {
    stats.as_ref().map(|s| PacketStats {
        good: s.good.unwrap_or(0),
        late: s.late.unwrap_or(0),
        lost: s.lost.unwrap_or(0),
        resync: s.resync.unwrap_or(0),
    })
}

impl HandleMessage for mumble_tcp::UserStats {
    fn handle(&self, ctx: &HandlerContext) {
        let Some(session) = self.session else { return };

        // Version / OS info (only present when the server includes details).
        let (version, os, os_version) = self
            .version
            .as_ref()
            .map(|v| {
                (
                    v.release.clone(),
                    v.os.clone(),
                    v.os_version.clone(),
                )
            })
            .unwrap_or_default();

        let address = self
            .address
            .as_ref()
            .map(|a| fancy_utils::net::format_ip_address(a));

        let rolling_stats = self.rolling_stats.as_ref().map(|rs| {
            RollingStatsPayload {
                time_window: rs.time_window.unwrap_or(0),
                from_client: extract_packet_stats(&rs.from_client)
                    .unwrap_or_default(),
                from_server: extract_packet_stats(&rs.from_server)
                    .unwrap_or_default(),
            }
        });

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
            version,
            os,
            os_version,
            address,
            from_client: extract_packet_stats(&self.from_client),
            from_server: extract_packet_stats(&self.from_server),
            rolling_stats,
        };

        ctx.emit("user-stats", payload);
    }
}
