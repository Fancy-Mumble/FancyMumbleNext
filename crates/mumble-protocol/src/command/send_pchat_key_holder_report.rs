use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Report to the server that a `cert_hash` now holds the E2EE key for a channel.
#[derive(Debug)]
pub struct SendPchatKeyHolderReport {
    /// The key-holder report payload.
    pub report: mumble_tcp::PchatKeyHolderReport,
}

impl CommandAction for SendPchatKeyHolderReport {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatKeyHolderReport(self.report.clone())],
            ..Default::default()
        }
    }
}
