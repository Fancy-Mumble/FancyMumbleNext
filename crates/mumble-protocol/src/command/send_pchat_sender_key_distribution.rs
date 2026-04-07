use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a Signal sender key distribution to the server for relay and storage.
#[derive(Debug)]
pub struct SendPchatSenderKeyDistribution {
    /// The channel this distribution is for.
    pub channel_id: u32,
    /// The raw SKDM bytes produced by the Signal bridge.
    pub distribution: Vec<u8>,
}

impl CommandAction for SendPchatSenderKeyDistribution {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatSenderKeyDistribution(
                mumble_tcp::PchatSenderKeyDistribution {
                    channel_id: Some(self.channel_id),
                    sender_hash: None, // server fills this on relay
                    distribution: Some(self.distribution.clone()),
                },
            )],
            ..Default::default()
        }
    }
}
