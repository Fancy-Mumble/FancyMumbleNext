use mumble_protocol::proto::mumble_tcp;
use tracing::info;

use super::{HandleMessage, HandlerContext};

impl HandleMessage for mumble_tcp::ServerConfig {
    fn handle(&self, ctx: &HandlerContext) {
        if let Ok(mut state) = ctx.shared.lock() {
            if let Some(len) = self.message_length {
                state.server_config.max_message_length = len;
            }
            if let Some(len) = self.image_message_length {
                // 0 means "no special limit" in the Mumble protocol;
                // keep the default (131072) rather than storing 0.
                if len > 0 {
                    state.server_config.max_image_message_length = len;
                }
            }
            if let Some(allow) = self.allow_html {
                state.server_config.allow_html = allow;
            }
            if let Some(max) = self.max_users {
                state.max_users = Some(max);
            }
            info!(
                msg_len = state.server_config.max_message_length,
                img_len = state.server_config.max_image_message_length,
                allow_html = state.server_config.allow_html,
                max_users = ?state.max_users,
                "server config received"
            );
        }
        ctx.emit_empty("server-config");
    }
}
