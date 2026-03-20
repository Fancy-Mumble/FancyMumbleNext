use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a text message to channels, users, or channel trees.
#[derive(Debug)]
pub struct SendTextMessage {
    /// Channel IDs to send the message to.
    pub channel_ids: Vec<u32>,
    /// User session IDs to send the message to directly.
    pub user_sessions: Vec<u32>,
    /// Channel tree roots to broadcast the message to (recursive).
    pub tree_ids: Vec<u32>,
    /// The HTML message body.
    pub message: String,
    /// Unique identifier for this message (Fancy Mumble extension).
    /// Ignored by legacy servers that don't recognise the field.
    pub message_id: Option<String>,
    /// Message timestamp as Unix epoch milliseconds (Fancy Mumble extension).
    /// Ignored by legacy servers that don't recognise the field.
    pub timestamp: Option<u64>,
}

impl CommandAction for SendTextMessage {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::TextMessage {
            session: self.user_sessions.clone(),
            channel_id: self.channel_ids.clone(),
            tree_id: self.tree_ids.clone(),
            message: self.message.clone(),
            message_id: self.message_id.clone(),
            timestamp: self.timestamp,
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::TextMessage(msg)],
            ..Default::default()
        }
    }
}
