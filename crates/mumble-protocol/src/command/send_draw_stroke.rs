use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a [`FancyDrawStroke`](mumble_tcp::FancyDrawStroke) to the server
/// for relay to all Fancy clients in the same channel.
///
/// The server sets `sender_session` before forwarding; the client
/// leaves it unset.
#[derive(Debug, Clone)]
pub struct SendDrawStroke {
    /// Channel where the screen share is taking place.
    pub channel_id: u32,
    /// Client-chosen stroke UUID grouping related point packets.
    pub stroke_id: String,
    /// Stroke colour packed as 0xAARRGGBB.
    pub color: u32,
    /// Stroke width in logical pixels at 1x scale.
    /// DEPRECATED: prefer `width_frac` for resolution-independent scaling.
    pub width: f32,
    /// Stroke width as a fraction of the shared content's pixel
    /// height. Resolution-independent. `None` means "use `width`".
    pub width_frac: Option<f32>,
    /// Normalised coordinate pairs [x0, y0, x1, y1, ...] in the range [0, 1].
    pub points: Vec<f32>,
    /// True when this packet ends the stroke (pointer-release / pen-up).
    pub is_end: bool,
    /// True to clear strokes in the channel.  By default removes only
    /// strokes sent by this user; set [`Self::clear_all`] together to
    /// wipe every sender's strokes (broadcaster-only operation).
    pub is_clear: bool,
    /// When set with `is_clear`, wipes ALL strokes in the channel.
    /// Reserved for the channel's active screen-sharer.
    pub clear_all: bool,
}

impl CommandAction for SendDrawStroke {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::FancyDrawStroke {
            sender_session: None,
            channel_id: Some(self.channel_id),
            stroke_id: Some(self.stroke_id.clone()),
            color: Some(self.color),
            width: Some(self.width),
            width_frac: self.width_frac,
            points: self.points.clone(),
            is_end: Some(self.is_end),
            is_clear: Some(self.is_clear),
            clear_all: Some(self.clear_all),
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::FancyDrawStroke(msg)],
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]

    use super::*;

    #[test]
    fn produces_draw_stroke_message() {
        let cmd = SendDrawStroke {
            channel_id: 3,
            stroke_id: "stroke-uuid-1".into(),
            color: 0xFF_FF_00_00,
            width: 4.0,
            width_frac: Some(0.005),
            points: vec![0.1, 0.2, 0.3, 0.4],
            is_end: true,
            is_clear: false,
            clear_all: false,
        };
        let output = cmd.execute(&ServerState::default());
        assert_eq!(output.tcp_messages.len(), 1);
        if let ControlMessage::FancyDrawStroke(m) = &output.tcp_messages[0] {
            assert_eq!(m.channel_id, Some(3));
            assert_eq!(m.stroke_id.as_deref(), Some("stroke-uuid-1"));
            assert_eq!(m.color, Some(0xFF_FF_00_00));
            assert_eq!(m.width_frac, Some(0.005));
            assert_eq!(m.is_end, Some(true));
            assert_eq!(m.is_clear, Some(false));
            assert_eq!(m.points, vec![0.1_f32, 0.2, 0.3, 0.4]);
        } else {
            panic!("expected FancyDrawStroke");
        }
    }

    #[test]
    fn clear_stroke_sets_flag() {
        let cmd = SendDrawStroke {
            channel_id: 1,
            stroke_id: "clear-id".into(),
            color: 0,
            width: 0.0,
            width_frac: None,
            points: vec![],
            is_end: false,
            is_clear: true,
            clear_all: false,
        };
        let output = cmd.execute(&ServerState::default());
        if let ControlMessage::FancyDrawStroke(m) = &output.tcp_messages[0] {
            assert_eq!(m.is_clear, Some(true));
        } else {
            panic!("expected FancyDrawStroke");
        }
    }

    #[test]
    fn clear_all_propagates_flag() {
        let cmd = SendDrawStroke {
            channel_id: 7,
            stroke_id: "clear-all-id".into(),
            color: 0,
            width: 0.0,
            width_frac: None,
            points: vec![],
            is_end: false,
            is_clear: true,
            clear_all: true,
        };
        let output = cmd.execute(&ServerState::default());
        if let ControlMessage::FancyDrawStroke(m) = &output.tcp_messages[0] {
            assert_eq!(m.is_clear, Some(true));
            assert_eq!(m.clear_all, Some(true));
        } else {
            panic!("expected FancyDrawStroke");
        }
    }
}
