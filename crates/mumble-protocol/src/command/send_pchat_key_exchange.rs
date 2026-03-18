use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send key exchange material to another client via the server relay.
#[derive(Debug)]
pub struct SendPchatKeyExchange {
    pub exchange: mumble_tcp::PchatKeyExchange,
}

impl CommandAction for SendPchatKeyExchange {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatKeyExchange(self.exchange.clone())],
            ..Default::default()
        }
    }
}
