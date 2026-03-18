use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Request the server to delete a channel.
///
/// The channel identified by `channel_id` will be removed.  The server
/// will reject the request unless the user has Write permission on that
/// channel.  On success the server broadcasts a `ChannelRemove` message
/// to all clients.
#[derive(Debug)]
pub struct DeleteChannel {
    pub channel_id: u32,
}

impl CommandAction for DeleteChannel {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::ChannelRemove(mumble_tcp::ChannelRemove {
                channel_id: self.channel_id,
            })],
            ..Default::default()
        }
    }
}
