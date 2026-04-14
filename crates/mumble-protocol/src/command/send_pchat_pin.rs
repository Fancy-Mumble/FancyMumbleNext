use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a pin or unpin request for a persisted chat message.
///
/// The server validates the sender, persists the pin state, and
/// broadcasts a [`PchatPinDeliver`](mumble_tcp::PchatPinDeliver)
/// to all Fancy clients in the channel.
#[derive(Debug)]
pub struct SendPchatPin {
    /// The pin request to send.
    pub message: mumble_tcp::PchatPin,
}

impl CommandAction for SendPchatPin {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatPin(self.message.clone())],
            ..Default::default()
        }
    }
}
