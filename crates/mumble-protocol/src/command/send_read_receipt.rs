use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a read receipt to the server, or query all read states for a channel.
#[derive(Debug)]
pub struct SendReadReceipt {
    /// Target channel.
    pub channel_id: u32,
    /// When `Some`, updates the watermark; when `None` with `query = true`, queries.
    pub last_read_message_id: Option<String>,
    /// When true, this is a query for all channel read states.
    pub query: bool,
    /// When set, the server response includes only states relevant to this message.
    pub query_message_id: Option<String>,
}

impl CommandAction for SendReadReceipt {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::FancyReadReceipt(
                mumble_tcp::FancyReadReceipt {
                    channel_id: Some(self.channel_id),
                    last_read_message_id: self.last_read_message_id.clone(),
                    timestamp: None,
                    query: if self.query { Some(true) } else { None },
                    query_message_id: self.query_message_id.clone(),
                },
            )],
            ..Default::default()
        }
    }
}
