use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Update a channel's name and/or description on the server.
///
/// Only fields set to `Some(...)` are included in the message;
/// the server ignores absent fields.  The caller must ensure the
/// user has the required permissions (Write / `MakeChannel`) before
/// sending.
#[derive(Debug)]
pub struct SetChannelState {
    pub channel_id: u32,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl CommandAction for SetChannelState {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::ChannelState {
            channel_id: Some(self.channel_id),
            name: self.name.clone(),
            description: self.description.clone(),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::ChannelState(msg)],
            ..Default::default()
        }
    }
}
