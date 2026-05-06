use mumble_protocol::proto::mumble_tcp;
use serde::Serialize;

use super::{HandleMessage, HandlerContext};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DrawStrokePayload {
    sender_session: u32,
    channel_id: u32,
    stroke_id: String,
    color: u32,
    width: f32,
    width_frac: Option<f32>,
    points: Vec<f32>,
    is_end: bool,
    is_clear: bool,
    clear_all: bool,
}

impl HandleMessage for mumble_tcp::FancyDrawStroke {
    fn handle(&self, ctx: &HandlerContext) {
        tracing::info!(
            target: "draw",
            sender_session = self.sender_session.unwrap_or(0),
            channel_id = self.channel_id.unwrap_or(0),
            stroke_id = %self.stroke_id.clone().unwrap_or_default(),
            coords = self.points.len(),
            is_end = self.is_end.unwrap_or(false),
            is_clear = self.is_clear.unwrap_or(false),
            clear_all = self.clear_all.unwrap_or(false),
            width_frac = ?self.width_frac,
            "rx FancyDrawStroke -> emit draw-stroke"
        );
        ctx.emit(
            "draw-stroke",
            DrawStrokePayload {
                sender_session: self.sender_session.unwrap_or(0),
                channel_id: self.channel_id.unwrap_or(0),
                stroke_id: self.stroke_id.clone().unwrap_or_default(),
                color: self.color.unwrap_or(0xFF_FF_00_00),
                width: self.width.unwrap_or(4.0),
                width_frac: self.width_frac,
                points: self.points.clone(),
                is_end: self.is_end.unwrap_or(false),
                is_clear: self.is_clear.unwrap_or(false),
                clear_all: self.clear_all.unwrap_or(false),
            },
        );
    }
}
