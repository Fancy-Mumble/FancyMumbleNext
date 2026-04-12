use mumble_protocol::proto::mumble_tcp;
use serde::Serialize;
use tracing::debug;

use super::{HandleMessage, HandlerContext};

#[derive(Serialize, Clone)]
struct CustomReactionPayload {
    shortcode: String,
    display: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

impl HandleMessage for mumble_tcp::FancyCustomReactionsConfig {
    fn handle(&self, ctx: &HandlerContext) {
        let reactions: Vec<CustomReactionPayload> = self
            .reactions
            .iter()
            .map(|r| CustomReactionPayload {
                shortcode: r.shortcode.clone().unwrap_or_default(),
                display: r.display.clone().unwrap_or_default(),
                label: r.label.clone(),
            })
            .collect();

        debug!(count = reactions.len(), "received FancyCustomReactionsConfig");

        ctx.emit("custom-reactions-config", reactions);
    }
}
