use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Notify the server that the local user is typing in a channel.
#[derive(Debug)]
pub struct SendTypingIndicator {
    /// Target channel.
    pub channel_id: u32,
}

impl CommandAction for SendTypingIndicator {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::FancyTypingIndicator(
                mumble_tcp::FancyTypingIndicator {
                    channel_id: Some(self.channel_id),
                    actor: None,
                },
            )],
            ..Default::default()
        }
    }
}
