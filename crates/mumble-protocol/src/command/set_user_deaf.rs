use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Admin-deafen (or undeafen) another user on the server.
///
/// This targets a specific session rather than the local user.
/// Requires appropriate server permissions.
#[derive(Debug)]
pub struct SetUserDeaf {
    pub session: u32,
    pub deafened: bool,
}

impl CommandAction for SetUserDeaf {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: Some(self.session),
            deaf: Some(self.deafened),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
