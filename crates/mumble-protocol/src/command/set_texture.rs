use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Set the current user's avatar texture (raw image bytes, typically PNG/JPEG).
///
/// Sends an empty `texture` to clear the avatar.
#[derive(Debug)]
pub struct SetTexture {
    /// Raw image bytes (typically PNG or JPEG). Empty to clear the avatar.
    pub texture: Vec<u8>,
}

impl CommandAction for SetTexture {
    fn execute(&self, state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: state.own_session(),
            texture: Some(self.texture.clone()),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
