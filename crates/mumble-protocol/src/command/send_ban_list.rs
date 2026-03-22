use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send an updated ban list to the server (replaces the entire list).
#[derive(Debug)]
pub struct SendBanList {
    /// The complete list of ban entries to send.
    pub bans: Vec<mumble_tcp::ban_list::BanEntry>,
}

impl CommandAction for SendBanList {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::BanList {
            bans: self.bans.clone(),
            query: Some(false),
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::BanList(msg)],
            ..Default::default()
        }
    }
}
