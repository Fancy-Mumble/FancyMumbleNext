use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Reply to the server's key-possession challenge with an HMAC proof.
#[derive(Debug)]
pub struct SendPchatKeyChallengeResponse {
    pub response: mumble_tcp::PchatKeyChallengeResponse,
}

impl CommandAction for SendPchatKeyChallengeResponse {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatKeyChallengeResponse(
                self.response.clone(),
            )],
            ..Default::default()
        }
    }
}
