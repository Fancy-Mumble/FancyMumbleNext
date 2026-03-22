use fancy_utils::net::format_ip_address;
use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::BanEntryPayload;

impl HandleMessage for mumble_tcp::BanList {
    fn handle(&self, ctx: &HandlerContext) {
        let bans: Vec<BanEntryPayload> = self
            .bans
            .iter()
            .map(|b| {
                let address_str = format_ip_address(&b.address);
                BanEntryPayload {
                    address: address_str,
                    mask: b.mask,
                    name: b.name.clone().unwrap_or_default(),
                    hash: b.hash.clone().unwrap_or_default(),
                    reason: b.reason.clone().unwrap_or_default(),
                    start: b.start.clone().unwrap_or_default(),
                    duration: b.duration.unwrap_or(0),
                }
            })
            .collect();
        ctx.emit("ban-list", bans);
    }
}
