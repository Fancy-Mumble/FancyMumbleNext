use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a key custodian countersignature for an epoch transition.
#[derive(Debug)]
pub struct SendPchatEpochCountersig {
    /// The countersignature payload.
    pub countersig: mumble_tcp::PchatEpochCountersig,
}

impl CommandAction for SendPchatEpochCountersig {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatEpochCountersig(self.countersig.clone())],
            ..Default::default()
        }
    }
}
