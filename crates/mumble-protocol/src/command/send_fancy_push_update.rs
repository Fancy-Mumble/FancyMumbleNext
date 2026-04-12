use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Update the per-channel push notification mute preferences on the server.
#[derive(Debug)]
pub struct SendFancyPushUpdate {
    /// Complete list of channel IDs muted for push notifications.
    pub muted_channels: Vec<u32>,
}

impl CommandAction for SendFancyPushUpdate {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::FancyPushUpdate(
                mumble_tcp::FancyPushUpdate {
                    muted_channels: self.muted_channels.clone(),
                },
            )],
            ..Default::default()
        }
    }
}
