use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Set the current user's comment.
#[derive(Debug)]
pub struct SetComment {
    /// The new comment HTML (empty string clears it).
    pub comment: String,
}

impl CommandAction for SetComment {
    fn execute(&self, state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: state.own_session(),
            comment: Some(self.comment.clone()),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
