use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a plugin data transmission to the server.
///
/// Used by FancyMumble for features like polls that are invisible to
/// legacy clients.  The `data_id` identifies the type of payload
/// (e.g. `"fancy-poll"`).
#[derive(Debug)]
pub struct SendPluginData {
    /// Recipient sessions - must list each target explicitly.
    /// The Mumble server only forwards to listed sessions; an empty
    /// list means nobody receives the message.
    pub receiver_sessions: Vec<u32>,
    /// Raw payload bytes (typically JSON).
    pub data: Vec<u8>,
    /// Plugin identifier string (e.g. "fancy-poll", "fancy-poll-vote").
    pub data_id: String,
}

impl CommandAction for SendPluginData {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::PluginDataTransmission {
            sender_session: None, // Server fills this in.
            receiver_sessions: self.receiver_sessions.clone(),
            data: Some(self.data.clone()),
            data_id: Some(self.data_id.clone()),
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::PluginDataTransmission(msg)],
            ..Default::default()
        }
    }
}
