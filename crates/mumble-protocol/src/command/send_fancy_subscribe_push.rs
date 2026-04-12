use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Register live push subscriptions with the server for connected clients.
///
/// The server computes the set of channels where the client has the
/// `SubscribePush` permission (0x2000) and routes `TextMessage`s from
/// those channels to this client.
#[derive(Debug)]
pub struct SendFancySubscribePush {
    /// Channel IDs the client wants to exclude from live delivery.
    pub muted_channels: Vec<u32>,
}

impl CommandAction for SendFancySubscribePush {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::FancySubscribePush(
                mumble_tcp::FancySubscribePush {
                    muted_channels: self.muted_channels.clone(),
                },
            )],
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn produces_subscribe_push_message() {
        let cmd = SendFancySubscribePush {
            muted_channels: vec![3, 7],
        };
        let state = ServerState::default();
        let output = cmd.execute(&state);

        assert_eq!(output.tcp_messages.len(), 1);
        match &output.tcp_messages[0] {
            ControlMessage::FancySubscribePush(msg) => {
                assert_eq!(msg.muted_channels, vec![3, 7]);
            }
            other => panic!("expected FancySubscribePush, got {other:?}"),
        }
    }

    #[test]
    fn empty_muted_channels() {
        let cmd = SendFancySubscribePush {
            muted_channels: vec![],
        };
        let state = ServerState::default();
        let output = cmd.execute(&state);

        assert_eq!(output.tcp_messages.len(), 1);
        match &output.tcp_messages[0] {
            ControlMessage::FancySubscribePush(msg) => {
                assert!(msg.muted_channels.is_empty());
            }
            other => panic!("expected FancySubscribePush, got {other:?}"),
        }
    }
}
