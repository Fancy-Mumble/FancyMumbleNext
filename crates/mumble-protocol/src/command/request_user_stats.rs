use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Request user statistics from the server.
#[derive(Debug)]
pub struct RequestUserStats {
    pub session: u32,
}

impl CommandAction for RequestUserStats {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserStats {
            session: Some(self.session),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserStats(msg)],
            ..Default::default()
        }
    }
}
