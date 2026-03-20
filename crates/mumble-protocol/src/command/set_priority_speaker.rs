use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Grant or revoke priority-speaker status for a user.
///
/// Priority speakers suppress the volume of other users while speaking.
/// Requires appropriate server permissions.
#[derive(Debug)]
pub struct SetPrioritySpeaker {
    /// Session ID of the target user.
    pub session: u32,
    /// `true` to grant, `false` to revoke priority-speaker status.
    pub priority: bool,
}

impl CommandAction for SetPrioritySpeaker {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: Some(self.session),
            priority_speaker: Some(self.priority),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
