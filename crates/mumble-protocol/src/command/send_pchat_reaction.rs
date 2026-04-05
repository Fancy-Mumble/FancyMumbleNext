use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a reaction (add or remove) on a persisted chat message.
///
/// The server validates the sender, persists the reaction, and
/// broadcasts a [`PchatReactionDeliver`](mumble_tcp::PchatReactionDeliver)
/// to all Fancy clients in the channel.
#[derive(Debug)]
pub struct SendPchatReaction {
    /// The reaction request to send.
    pub message: mumble_tcp::PchatReaction,
}

impl CommandAction for SendPchatReaction {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatReaction(self.message.clone())],
            ..Default::default()
        }
    }
}
