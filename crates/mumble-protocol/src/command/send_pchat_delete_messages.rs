use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Request deletion of persisted chat messages.
///
/// The server deletes matching messages and acknowledges with
/// [`PchatAck`](mumble_tcp::PchatAck) (status `PCHAT_ACK_DELETED`),
/// then broadcasts the same [`PchatDeleteMessages`](mumble_tcp::PchatDeleteMessages)
/// to other verified sessions so they can evict locally.
#[derive(Debug)]
pub struct SendPchatDeleteMessages {
    /// The delete request to send.
    pub message: mumble_tcp::PchatDeleteMessages,
}

impl CommandAction for SendPchatDeleteMessages {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatDeleteMessages(self.message.clone())],
            ..Default::default()
        }
    }
}
