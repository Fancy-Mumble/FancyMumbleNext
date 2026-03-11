use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Move self to a different channel.
#[derive(Debug)]
pub struct JoinChannel {
    pub channel_id: u32,
}

impl CommandAction for JoinChannel {
    fn execute(&self, state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: state.own_session(),
            channel_id: Some(self.channel_id),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
