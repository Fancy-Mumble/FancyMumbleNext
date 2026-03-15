use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Admin-mute (or unmute) another user on the server.
///
/// This targets a specific session rather than the local user.
/// Requires appropriate server permissions.
#[derive(Debug)]
pub struct SetUserMute {
    pub session: u32,
    pub muted: bool,
}

impl CommandAction for SetUserMute {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: Some(self.session),
            mute: Some(self.muted),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
