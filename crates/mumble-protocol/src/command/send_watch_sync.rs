use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send a [`FancyWatchSync`](mumble_tcp::FancyWatchSync) event to other
/// participants of a watch-together session.
///
/// Targeting is controlled by the message itself: the `FancyMumble` server
/// relays the event to every channel member who has subscribed to it via
/// [`SendFancySubscribePush`](crate::command::SendFancySubscribePush)
/// (same path as `FancyTypingIndicator`).  On a legacy server the
/// `LegacyCodec` automatically wraps the message in `PluginData` for the
/// `receiver_sessions` listed by the caller of that codec's encode path.
#[derive(Debug)]
pub struct SendWatchSync {
    /// The watch-sync event to deliver.
    pub message: mumble_tcp::FancyWatchSync,
}

impl CommandAction for SendWatchSync {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::FancyWatchSync(self.message.clone())],
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]

    use super::*;
    use crate::proto::mumble_tcp::fancy_watch_sync::{Event, Start};

    #[test]
    fn produces_watch_sync_message() {
        let inner = mumble_tcp::FancyWatchSync {
            session_id: Some("sess-1".into()),
            actor: None,
            event: Some(Event::Start(Start {
                channel_id: Some(7),
                source_url: Some("https://example.com/v.mp4".into()),
                source_kind: Some(0),
                title: Some("Demo".into()),
                host_session: Some(42),
            })),
        };
        let cmd = SendWatchSync {
            message: inner.clone(),
        };
        let state = ServerState::default();
        let out = cmd.execute(&state);
        assert_eq!(out.tcp_messages.len(), 1);
        match &out.tcp_messages[0] {
            ControlMessage::FancyWatchSync(m) => {
                assert_eq!(m.session_id.as_deref(), Some("sess-1"));
                assert!(matches!(m.event, Some(Event::Start(_))));
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }
}
