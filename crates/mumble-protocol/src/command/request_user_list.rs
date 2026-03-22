use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Request the registered user list from the server.
#[derive(Debug)]
pub struct RequestUserList;

impl CommandAction for RequestUserList {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserList {
            users: Vec::new(),
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserList(msg)],
            ..Default::default()
        }
    }
}
