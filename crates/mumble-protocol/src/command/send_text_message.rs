use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a text message to channels, users, or channel trees.
#[derive(Debug)]
pub struct SendTextMessage {
    pub channel_ids: Vec<u32>,
    pub user_sessions: Vec<u32>,
    pub tree_ids: Vec<u32>,
    pub message: String,
}

impl CommandAction for SendTextMessage {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::TextMessage {
            session: self.user_sessions.clone(),
            channel_id: self.channel_ids.clone(),
            tree_id: self.tree_ids.clone(),
            message: self.message.clone(),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::TextMessage(msg)],
            ..Default::default()
        }
    }
}
