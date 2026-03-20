use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Ask the server for the list of key holders for a channel.
#[derive(Debug)]
pub struct SendPchatKeyHoldersQuery {
    /// The key-holders query payload.
    pub query: mumble_tcp::PchatKeyHoldersQuery,
}

impl CommandAction for SendPchatKeyHoldersQuery {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatKeyHoldersQuery(self.query)],            
            ..Default::default()
        }
    }
}
