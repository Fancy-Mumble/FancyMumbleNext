use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Register an FCM device token with the server for push notifications.
#[derive(Debug)]
pub struct SendFancyPushRegister {
    /// FCM device registration token.
    pub token: String,
    /// Channel IDs the client has muted for push notifications.
    pub muted_channels: Vec<u32>,
}

impl CommandAction for SendFancyPushRegister {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::FancyPushRegister(
                mumble_tcp::FancyPushRegister {
                    token: Some(self.token.clone()),
                    muted_channels: self.muted_channels.clone(),
                },
            )],
            ..Default::default()
        }
    }
}
