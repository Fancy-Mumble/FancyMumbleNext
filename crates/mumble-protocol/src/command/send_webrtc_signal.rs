use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a WebRTC screen-sharing signaling message to the server.
///
/// The server stamps `sender_session` and relays the message to
/// `target_session` (or broadcasts to channel members when 0).
#[derive(Debug)]
pub struct SendWebRtcSignal {
    /// Recipient session (0 = broadcast to sender's channel).
    pub target_session: u32,
    /// Signal type (maps to `WebRtcSignal::SignalType` enum).
    pub signal_type: i32,
    /// SDP or JSON-encoded ICE candidate string.
    pub payload: String,
}

impl CommandAction for SendWebRtcSignal {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::WebRtcSignal {
            target_session: Some(self.target_session),
            sender_session: None, // Server fills this in.
            signal_type: Some(self.signal_type),
            payload: Some(self.payload.clone()),
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::WebRtcSignal(msg)],
            ..Default::default()
        }
    }
}
