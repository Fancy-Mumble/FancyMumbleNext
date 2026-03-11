use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Start or stop listening to one or more channels.
///
/// Listening to a channel lets you receive text messages and audio
/// from that channel without being physically present in it.
#[derive(Debug)]
pub struct ChannelListen {
    /// Channel IDs to start listening to.
    pub add: Vec<u32>,
    /// Channel IDs to stop listening to.
    pub remove: Vec<u32>,
}

impl CommandAction for ChannelListen {
    fn execute(&self, state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: state.own_session(),
            listening_channel_add: self.add.clone(),
            listening_channel_remove: self.remove.clone(),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}
