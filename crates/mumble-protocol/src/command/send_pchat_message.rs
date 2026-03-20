use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send an encrypted persistent chat message to the server for storage and relay.
#[derive(Debug)]
pub struct SendPchatMessage {
    /// The encrypted persistent chat message to send.
    pub message: mumble_tcp::PchatMessage,
}

impl CommandAction for SendPchatMessage {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatMessage(self.message.clone())],
            ..Default::default()
        }
    }
}
