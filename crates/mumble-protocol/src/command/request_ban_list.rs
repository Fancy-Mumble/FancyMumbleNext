use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Request the ban list from the server.
#[derive(Debug)]
pub struct RequestBanList;

impl CommandAction for RequestBanList {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::BanList {
            query: Some(true),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::BanList(msg)],
            ..Default::default()
        }
    }
}
