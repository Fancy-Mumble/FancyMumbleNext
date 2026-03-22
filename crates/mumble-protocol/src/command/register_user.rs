use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Register a user on the server using their current certificate.
///
/// Sends a `UserState` with `user_id = 0` for the target session,
/// which tells the server to register that user. The server requires
/// `ChanACL::Register` permission on the root channel (or
/// `SelfRegister` when targeting yourself).
#[derive(Debug)]
pub struct RegisterUser {
    /// Session ID of the user to register.
    pub session: u32,
}

impl CommandAction for RegisterUser {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: Some(self.session),
            user_id: Some(0),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
