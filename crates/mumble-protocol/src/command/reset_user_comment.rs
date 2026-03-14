use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Admin-reset another user's comment to empty.
///
/// Requires appropriate server permissions.
#[derive(Debug)]
pub struct ResetUserComment {
    pub session: u32,
}

impl CommandAction for ResetUserComment {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: Some(self.session),
            comment: Some(String::new()),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
