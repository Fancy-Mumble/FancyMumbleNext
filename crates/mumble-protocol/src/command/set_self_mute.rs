use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Set self-mute state.
///
/// Unmuting also clears self-deaf automatically.
#[derive(Debug)]
pub struct SetSelfMute {
    pub muted: bool,
}

impl CommandAction for SetSelfMute {
    fn execute(&self, state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: state.own_session(),
            self_mute: Some(self.muted),
            self_deaf: if !self.muted { Some(false) } else { None },
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
