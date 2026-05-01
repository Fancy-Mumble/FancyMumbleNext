use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Move another user into a different channel (admin action).
///
/// Sent as a `UserState { session, channel_id }` message.  Requires
/// the `Move` permission on both the source and destination channels
/// (or `MoveAll` server-wide).  Self-moves should use [`JoinChannel`]
/// instead so the protocol intent is unambiguous.
///
/// [`JoinChannel`]: crate::command::JoinChannel
#[derive(Debug)]
pub struct MoveUser {
    /// Session ID of the user to move.
    pub session: u32,
    /// Destination channel ID.
    pub channel_id: u32,
}

impl CommandAction for MoveUser {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::UserState {
            session: Some(self.session),
            channel_id: Some(self.channel_id),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserState(msg)],
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ControlMessage;
    use crate::state::ServerState;

    #[test]
    fn move_user_emits_user_state_with_session_and_channel() {
        let cmd = MoveUser { session: 7, channel_id: 42 };
        let state = ServerState::new();
        let out = cmd.execute(&state);
        assert_eq!(out.tcp_messages.len(), 1);
        let ControlMessage::UserState(us) = &out.tcp_messages[0] else {
            panic!("expected UserState");
        };
        assert_eq!(us.session, Some(7));
        assert_eq!(us.channel_id, Some(42));
        assert!(us.mute.is_none());
        assert!(us.deaf.is_none());
    }
}
