use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Announce E2EE identity public keys to the server for relay to other clients.
#[derive(Debug)]
pub struct SendPchatKeyAnnounce {
    /// The key announcement payload.
    pub announce: mumble_tcp::PchatKeyAnnounce,
}

impl CommandAction for SendPchatKeyAnnounce {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatKeyAnnounce(self.announce.clone())],
            ..Default::default()
        }
    }
}
