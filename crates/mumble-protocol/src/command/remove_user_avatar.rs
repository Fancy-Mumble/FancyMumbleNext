use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Admin-remove another user's avatar texture.
///
/// Sends an empty texture to the server, clearing the avatar.
/// Requires appropriate server permissions.
#[derive(Debug)]
pub struct RemoveUserAvatar {
    pub session: u32,
}

impl CommandAction for RemoveUserAvatar {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: Some(self.session),
            texture: Some(Vec::new()),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
