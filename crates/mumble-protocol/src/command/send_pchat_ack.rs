use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a `PchatAck` to the server (e.g. to acknowledge receipt of
/// offline-queued messages so the server can delete them).
#[derive(Debug)]
pub struct SendPchatAck {
    /// The ack message to send.
    pub ack: mumble_tcp::PchatAck,
}

impl CommandAction for SendPchatAck {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::PchatAck(self.ack.clone())],
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn send_pchat_ack_produces_ack_message() {
        let ack = mumble_tcp::PchatAck {
            message_ids: vec!["m1".into(), "m2".into()],
            status: Some(0),
            reason: None,
            channel_id: Some(5),
        };
        let cmd = SendPchatAck { ack: ack.clone() };
        let state = ServerState::default();
        let output = cmd.execute(&state);

        assert_eq!(output.tcp_messages.len(), 1);
        match &output.tcp_messages[0] {
            ControlMessage::PchatAck(a) => {
                assert_eq!(a.message_ids, vec!["m1", "m2"]);
                assert_eq!(a.channel_id, Some(5));
            }
            other => panic!("expected PchatAck, got {other:?}"),
        }
        assert!(!output.disconnect);
    }
}
