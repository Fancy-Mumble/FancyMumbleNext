//! Self-describing commands using the Command pattern.
//!
//! Each command lives in its own file, carries its own data, and knows
//! how to produce the protocol output it needs. Adding a new command
//! means creating a new file + struct - no existing code changes.

mod authenticate;
mod ban_user;
mod channel_listen;
mod delete_channel;
mod disconnect;
mod join_channel;
mod kick_user;
mod permission_query;
mod remove_user_avatar;
mod request_blob;
mod request_ban_list;
mod request_user_stats;
mod reset_user_comment;
mod send_audio;
mod send_ping;
mod send_plugin_data;
mod send_text_message;
mod send_pchat_message;
mod send_pchat_fetch;
mod send_pchat_key_announce;
mod send_pchat_key_exchange;
mod send_pchat_epoch_countersig;
mod set_channel_state;
mod set_comment;
mod set_priority_speaker;
mod set_self_deaf;
mod set_self_mute;
mod set_texture;
mod set_user_deaf;
mod set_user_mute;
mod set_voice_target;

// Re-export the core trait, output type, and boxed alias.
pub use self::core::{BoxedCommand, CommandAction, CommandOutput};

// Re-export every concrete command for ergonomic access.
pub use authenticate::Authenticate;
pub use ban_user::BanUser;
pub use channel_listen::ChannelListen;
pub use delete_channel::DeleteChannel;
pub use disconnect::Disconnect;
pub use join_channel::JoinChannel;
pub use kick_user::KickUser;
pub use permission_query::PermissionQuery;
pub use remove_user_avatar::RemoveUserAvatar;
pub use request_blob::RequestBlob;
pub use request_ban_list::RequestBanList;
pub use request_user_stats::RequestUserStats;
pub use reset_user_comment::ResetUserComment;
pub use send_audio::SendAudio;
pub use send_ping::SendPing;
pub use send_plugin_data::SendPluginData;
pub use send_text_message::SendTextMessage;
pub use send_pchat_message::SendPchatMessage;
pub use send_pchat_fetch::SendPchatFetch;
pub use send_pchat_key_announce::SendPchatKeyAnnounce;
pub use send_pchat_key_exchange::SendPchatKeyExchange;
pub use send_pchat_epoch_countersig::SendPchatEpochCountersig;
pub use set_channel_state::SetChannelState;
pub use set_comment::SetComment;
pub use set_priority_speaker::SetPrioritySpeaker;
pub use set_self_deaf::SetSelfDeaf;
pub use set_self_mute::SetSelfMute;
pub use set_texture::SetTexture;
pub use set_user_deaf::SetUserDeaf;
pub use set_user_mute::SetUserMute;
pub use set_voice_target::{SetVoiceTarget, VoiceTargetEntry};

mod core {
    use std::fmt::Debug;

    use crate::message::{ControlMessage, UdpMessage};
    use crate::state::ServerState;

    /// The output(s) a command wants to send after executing.
    #[derive(Debug, Default)]
    pub struct CommandOutput {
        /// Control messages to send over TCP.
        pub tcp_messages: Vec<ControlMessage>,
        /// Audio/ping messages to send over UDP.
        pub udp_messages: Vec<UdpMessage>,
        /// Whether the client should disconnect after processing.
        pub disconnect: bool,
    }

    /// Trait implemented by every user-facing command.
    ///
    /// Each struct owns its data and converts itself into protocol messages
    /// when [`execute`](CommandAction::execute) is called - keeping knowledge
    /// about the wire format inside the command, not in a central dispatcher.
    pub trait CommandAction: Debug + Send + Sync + 'static {
        /// Produce the protocol messages this command needs to send.
        ///
        /// Receives a read-only view of the current [`ServerState`] so commands
        /// can inspect the session ID, current channel, etc.
        fn execute(&self, state: &ServerState) -> CommandOutput;
    }

    /// Type-erased command stored in the work queue.
    pub type BoxedCommand = Box<dyn CommandAction>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{ControlMessage, UdpMessage};
    use crate::proto::mumble_tcp;
    use crate::state::ServerState;

    fn state_with_session(session_id: u32) -> ServerState {
        let mut state = ServerState::new();
        state.apply_server_sync(&mumble_tcp::ServerSync {
            session: Some(session_id),
            ..Default::default()
        });
        state
    }

    #[test]
    fn authenticate_produces_authenticate_message() {
        let cmd = Authenticate {
            username: "TestUser".into(),
            password: Some("secret".into()),
            tokens: vec!["token1".into()],
        };
        let state = ServerState::new();
        let output = cmd.execute(&state);

        assert_eq!(output.tcp_messages.len(), 1);
        assert!(!output.disconnect);
        match &output.tcp_messages[0] {
            ControlMessage::Authenticate(auth) => {
                assert_eq!(auth.username.as_deref(), Some("TestUser"));
                assert_eq!(auth.password.as_deref(), Some("secret"));
                assert_eq!(auth.tokens, vec!["token1"]);
                assert_eq!(auth.opus, Some(true));
            }
            other => panic!("expected Authenticate, got {other:?}"),
        }
    }

    #[test]
    fn disconnect_sets_disconnect_flag() {
        let cmd = Disconnect;
        let state = ServerState::new();
        let output = cmd.execute(&state);

        assert!(output.disconnect);
        assert!(output.tcp_messages.is_empty());
        assert!(output.udp_messages.is_empty());
    }

    #[test]
    fn join_channel_uses_session_id() {
        let cmd = JoinChannel { channel_id: 3 };
        let state = state_with_session(42);
        let output = cmd.execute(&state);

        assert_eq!(output.tcp_messages.len(), 1);
        match &output.tcp_messages[0] {
            ControlMessage::UserState(us) => {
                assert_eq!(us.session, Some(42));
                assert_eq!(us.channel_id, Some(3));
            }
            other => panic!("expected UserState, got {other:?}"),
        }
    }

    #[test]
    fn send_text_message_to_channels() {
        let cmd = SendTextMessage {
            channel_ids: vec![0, 1],
            user_sessions: vec![],
            tree_ids: vec![],
            message: "Hello world".into(),
            message_id: None,
            timestamp: None,
        };
        let state = ServerState::new();
        let output = cmd.execute(&state);

        assert_eq!(output.tcp_messages.len(), 1);
        match &output.tcp_messages[0] {
            ControlMessage::TextMessage(tm) => {
                assert_eq!(tm.channel_id, vec![0, 1]);
                assert_eq!(tm.message, "Hello world");
                assert!(tm.session.is_empty());
            }
            other => panic!("expected TextMessage, got {other:?}"),
        }
    }

    #[test]
    fn send_text_message_to_users() {
        let cmd = SendTextMessage {
            channel_ids: vec![],
            user_sessions: vec![10, 20],
            tree_ids: vec![],
            message: "DM".into(),
            message_id: None,
            timestamp: None,
        };
        let output = cmd.execute(&ServerState::new());
        match &output.tcp_messages[0] {
            ControlMessage::TextMessage(tm) => {
                assert_eq!(tm.session, vec![10, 20]);
                assert_eq!(tm.message, "DM");
            }
            other => panic!("expected TextMessage, got {other:?}"),
        }
    }

    #[test]
    fn set_comment_uses_session() {
        let cmd = SetComment {
            comment: "Away".into(),
        };
        let state = state_with_session(7);
        let output = cmd.execute(&state);
        match &output.tcp_messages[0] {
            ControlMessage::UserState(us) => {
                assert_eq!(us.session, Some(7));
                assert_eq!(us.comment.as_deref(), Some("Away"));
            }
            other => panic!("expected UserState, got {other:?}"),
        }
    }

    #[test]
    fn set_self_mute() {
        let cmd = SetSelfMute { muted: true };
        let state = state_with_session(5);
        let output = cmd.execute(&state);
        match &output.tcp_messages[0] {
            ControlMessage::UserState(us) => {
                assert_eq!(us.self_mute, Some(true));
                // Muting should not auto-clear deaf
                assert!(us.self_deaf.is_none());
            }
            other => panic!("expected UserState, got {other:?}"),
        }
    }

    #[test]
    fn set_self_unmute_clears_deaf() {
        let cmd = SetSelfMute { muted: false };
        let state = state_with_session(5);
        let output = cmd.execute(&state);
        match &output.tcp_messages[0] {
            ControlMessage::UserState(us) => {
                assert_eq!(us.self_mute, Some(false));
                assert_eq!(us.self_deaf, Some(false)); // undeafen on unmute
            }
            other => panic!("expected UserState, got {other:?}"),
        }
    }

    #[test]
    fn set_self_deaf_implies_mute() {
        let cmd = SetSelfDeaf { deafened: true };
        let state = state_with_session(5);
        let output = cmd.execute(&state);
        match &output.tcp_messages[0] {
            ControlMessage::UserState(us) => {
                assert_eq!(us.self_deaf, Some(true));
                assert_eq!(us.self_mute, Some(true)); // auto-mute on deafen
            }
            other => panic!("expected UserState, got {other:?}"),
        }
    }

    #[test]
    fn channel_listen_add_and_remove() {
        let cmd = ChannelListen {
            add: vec![1, 2],
            remove: vec![3],
        };
        let state = state_with_session(10);
        let output = cmd.execute(&state);
        match &output.tcp_messages[0] {
            ControlMessage::UserState(us) => {
                assert_eq!(us.session, Some(10));
                assert_eq!(us.listening_channel_add, vec![1, 2]);
                assert_eq!(us.listening_channel_remove, vec![3]);
            }
            other => panic!("expected UserState, got {other:?}"),
        }
    }

    #[test]
    fn ban_user_produces_user_remove_with_ban() {
        let cmd = BanUser {
            session: 99,
            reason: Some("Spam".into()),
        };
        let output = cmd.execute(&ServerState::new());
        match &output.tcp_messages[0] {
            ControlMessage::UserRemove(ur) => {
                assert_eq!(ur.session, 99);
                assert_eq!(ur.ban, Some(true));
                assert_eq!(ur.reason.as_deref(), Some("Spam"));
            }
            other => panic!("expected UserRemove, got {other:?}"),
        }
    }

    #[test]
    fn kick_user_no_ban() {
        let cmd = KickUser {
            session: 50,
            reason: None,
        };
        let output = cmd.execute(&ServerState::new());
        match &output.tcp_messages[0] {
            ControlMessage::UserRemove(ur) => {
                assert_eq!(ur.session, 50);
                assert_eq!(ur.ban, Some(false));
            }
            other => panic!("expected UserRemove, got {other:?}"),
        }
    }

    #[test]
    fn send_audio_produces_udp_message() {
        let cmd = SendAudio {
            opus_data: vec![0xDE, 0xAD],
            target: 0,
            frame_number: 42,
            positional_data: None,
            is_terminator: false,
        };
        let output = cmd.execute(&ServerState::new());
        assert!(output.tcp_messages.is_empty());
        assert_eq!(output.udp_messages.len(), 1);
        match &output.udp_messages[0] {
            UdpMessage::Audio(a) => {
                assert_eq!(a.opus_data, vec![0xDE, 0xAD]);
                assert_eq!(a.frame_number, 42);
                assert!(!a.is_terminator);
            }
            other => panic!("expected Audio, got {other:?}"),
        }
    }

    #[test]
    fn send_audio_with_position() {
        let cmd = SendAudio {
            opus_data: vec![],
            target: 1,
            frame_number: 0,
            positional_data: Some([1.0, 2.0, 3.0]),
            is_terminator: true,
        };
        let output = cmd.execute(&ServerState::new());
        match &output.udp_messages[0] {
            UdpMessage::Audio(a) => {
                assert_eq!(a.positional_data, vec![1.0, 2.0, 3.0]);
                assert!(a.is_terminator);
            }
            other => panic!("expected Audio, got {other:?}"),
        }
    }

    #[test]
    fn request_ban_list_sends_query() {
        let output = RequestBanList.execute(&ServerState::new());
        match &output.tcp_messages[0] {
            ControlMessage::BanList(bl) => {
                assert_eq!(bl.query, Some(true));
            }
            other => panic!("expected BanList, got {other:?}"),
        }
    }

    #[test]
    fn request_user_stats() {
        let cmd = RequestUserStats { session: 77 };
        let output = cmd.execute(&ServerState::new());
        match &output.tcp_messages[0] {
            ControlMessage::UserStats(us) => {
                assert_eq!(us.session, Some(77));
            }
            other => panic!("expected UserStats, got {other:?}"),
        }
    }

    #[test]
    fn set_voice_target() {
        let cmd = SetVoiceTarget {
            id: 1,
            targets: vec![VoiceTargetEntry {
                sessions: vec![10, 20],
                channel_id: Some(5),
                group: None,
                links: true,
                children: false,
            }],
        };
        let output = cmd.execute(&ServerState::new());
        match &output.tcp_messages[0] {
            ControlMessage::VoiceTarget(vt) => {
                assert_eq!(vt.id, Some(1));
                assert_eq!(vt.targets.len(), 1);
                assert_eq!(vt.targets[0].session, vec![10, 20]);
                assert_eq!(vt.targets[0].channel_id, Some(5));
                assert_eq!(vt.targets[0].links, Some(true));
                assert!(vt.targets[0].children.is_none()); // false -> None
            }
            other => panic!("expected VoiceTarget, got {other:?}"),
        }
    }

    // -- SendPluginData tests --------------------------------------

    #[test]
    fn send_plugin_data_produces_correct_message() {
        let cmd = SendPluginData {
            receiver_sessions: vec![10, 20, 30],
            data: b"test payload".to_vec(),
            data_id: "fancy-poll".into(),
        };
        let output = cmd.execute(&ServerState::new());

        assert_eq!(output.tcp_messages.len(), 1);
        assert!(!output.disconnect);
        assert!(output.udp_messages.is_empty());

        match &output.tcp_messages[0] {
            ControlMessage::PluginDataTransmission(pd) => {
                // Sender is None - the server fills it in.
                assert!(pd.sender_session.is_none());
                assert_eq!(pd.receiver_sessions, vec![10, 20, 30]);
                assert_eq!(pd.data.as_deref(), Some(b"test payload".as_slice()));
                assert_eq!(pd.data_id.as_deref(), Some("fancy-poll"));
            }
            other => panic!("expected PluginDataTransmission, got {other:?}"),
        }
    }

    #[test]
    fn send_plugin_data_empty_receivers_sends_to_nobody() {
        let cmd = SendPluginData {
            receiver_sessions: vec![],
            data: b"{}".to_vec(),
            data_id: "fancy-poll".into(),
        };
        let output = cmd.execute(&ServerState::new());

        match &output.tcp_messages[0] {
            ControlMessage::PluginDataTransmission(pd) => {
                assert!(
                    pd.receiver_sessions.is_empty(),
                    "empty receivers means nobody receives the message on the server"
                );
            }
            other => panic!("expected PluginDataTransmission, got {other:?}"),
        }
    }

    #[test]
    fn send_plugin_data_preserves_json_payload() {
        let json = r#"{"type":"poll","id":"abc-123","question":"Favourite?","options":["A","B"]}"#;
        let cmd = SendPluginData {
            receiver_sessions: vec![5],
            data: json.as_bytes().to_vec(),
            data_id: "fancy-poll".into(),
        };
        let output = cmd.execute(&ServerState::new());

        match &output.tcp_messages[0] {
            ControlMessage::PluginDataTransmission(pd) => {
                let payload = std::str::from_utf8(pd.data.as_deref().unwrap()).unwrap();
                assert_eq!(payload, json);
            }
            other => panic!("expected PluginDataTransmission, got {other:?}"),
        }
    }

    #[test]
    fn send_plugin_data_vote_message() {
        let json = r#"{"type":"poll_vote","pollId":"abc-123","selected":[0,2],"voter":42,"voterName":"Bob"}"#;
        let cmd = SendPluginData {
            receiver_sessions: vec![10],
            data: json.as_bytes().to_vec(),
            data_id: "fancy-poll-vote".into(),
        };
        let output = cmd.execute(&ServerState::new());

        match &output.tcp_messages[0] {
            ControlMessage::PluginDataTransmission(pd) => {
                assert_eq!(pd.data_id.as_deref(), Some("fancy-poll-vote"));
                let payload = std::str::from_utf8(pd.data.as_deref().unwrap()).unwrap();
                assert!(payload.contains("poll_vote"));
                assert!(payload.contains("abc-123"));
            }
            other => panic!("expected PluginDataTransmission, got {other:?}"),
        }
    }
}
