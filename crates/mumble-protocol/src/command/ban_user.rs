use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Ban a user from the server.
#[derive(Debug)]
pub struct BanUser {
    /// Session ID of the user to ban.
    pub session: u32,
    /// Optional human-readable ban reason.
    pub reason: Option<String>,
}

impl CommandAction for BanUser {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserRemove {
            session: self.session,
            reason: self.reason.clone(),
            ban: Some(true),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserRemove(msg)],
            ..Default::default()
        }
    }
}
