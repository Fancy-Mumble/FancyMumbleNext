use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Request stored persistent chat messages from the server.
#[derive(Debug)]
pub struct SendPchatFetch {
    pub fetch: mumble_tcp::PchatFetch,
}

impl CommandAction for SendPchatFetch {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatFetch(self.fetch.clone())],
            ..Default::default()
        }
    }
}
