use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Set self-deaf state.
///
/// Deafening implies self-mute.
#[derive(Debug)]
pub struct SetSelfDeaf {
    pub deafened: bool,
}

impl CommandAction for SetSelfDeaf {
    fn execute(&self, state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: state.own_session(),
            self_deaf: Some(self.deafened),
            self_mute: if self.deafened { Some(true) } else { None },
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
