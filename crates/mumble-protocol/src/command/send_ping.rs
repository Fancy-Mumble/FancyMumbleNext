use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a TCP ping with a specific timestamp for latency measurement.
#[derive(Debug)]
pub struct SendPing {
    pub timestamp: u64,
}

impl CommandAction for SendPing {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::Ping(mumble_tcp::Ping {
                timestamp: Some(self.timestamp),
                ..Default::default()
            })],
            ..Default::default()
        }
    }
}
