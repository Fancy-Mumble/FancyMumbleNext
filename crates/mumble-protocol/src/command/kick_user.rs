use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Kick a user from the server.
#[derive(Debug)]
pub struct KickUser {
    pub session: u32,
    pub reason: Option<String>,
}

impl CommandAction for KickUser {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserRemove {
            session: self.session,
            reason: self.reason.clone(),
            ban: Some(false),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserRemove(msg)],
            ..Default::default()
        }
    }
}
