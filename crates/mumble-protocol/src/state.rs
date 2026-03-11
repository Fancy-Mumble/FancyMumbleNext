//! Tracked server state - users, channels, and connection metadata.
//!
//! The client orchestrator updates this state as server messages arrive.
//! External consumers can query it through the public API.

use std::collections::HashMap;

/// Snapshot of a connected user.
#[derive(Debug, Clone)]
pub struct User {
    pub session: u32,
    pub name: String,
    pub channel_id: u32,
    pub mute: bool,
    pub deaf: bool,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub comment: String,
    pub texture: Vec<u8>,
    pub hash: String,
}

/// Snapshot of a channel on the server.
#[derive(Debug, Clone)]
pub struct Channel {
    pub channel_id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
    pub description: String,
    pub position: i32,
    pub temporary: bool,
    pub max_users: u32,
}

/// Connection-level metadata received during handshake.
#[derive(Debug, Clone, Default)]
pub struct ConnectionInfo {
    /// Our own session ID assigned by the server.
    pub session_id: Option<u32>,
    /// Maximum bandwidth allowed by the server.
    pub max_bandwidth: Option<u32>,
    /// Server welcome text.
    pub welcome_text: Option<String>,
}

/// Aggregated server state maintained by the client.
#[derive(Debug, Default)]
pub struct ServerState {
    pub connection: ConnectionInfo,
    pub users: HashMap<u32, User>,
    pub channels: HashMap<u32, Channel>,
}

impl ServerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a `UserState` update from the server.
    pub fn apply_user_state(&mut self, state: &crate::proto::mumble_tcp::UserState) {
        let Some(session) = state.session else {
            return;
        };

        let user = self.users.entry(session).or_insert_with(|| User {
            session,
            name: String::new(),
            channel_id: 0,
            mute: false,
            deaf: false,
            self_mute: false,
            self_deaf: false,
            comment: String::new(),
            texture: Vec::new(),
            hash: String::new(),
        });

        if let Some(ref name) = state.name {
            user.name = name.clone();
        }
        if let Some(channel_id) = state.channel_id {
            user.channel_id = channel_id;
        }
        if let Some(mute) = state.mute {
            user.mute = mute;
        }
        if let Some(deaf) = state.deaf {
            user.deaf = deaf;
        }
        if let Some(self_mute) = state.self_mute {
            user.self_mute = self_mute;
        }
        if let Some(self_deaf) = state.self_deaf {
            user.self_deaf = self_deaf;
        }
        if let Some(ref comment) = state.comment {
            user.comment = comment.clone();
        }
        if let Some(ref texture) = state.texture {
            user.texture = texture.clone();
        }
        if let Some(ref hash) = state.hash {
            user.hash = hash.clone();
        }
    }

    /// Remove a user from state.
    pub fn remove_user(&mut self, session: u32) {
        self.users.remove(&session);
    }

    /// Apply a `ChannelState` update from the server.
    pub fn apply_channel_state(&mut self, state: &crate::proto::mumble_tcp::ChannelState) {
        let Some(channel_id) = state.channel_id else {
            return;
        };

        let channel = self.channels.entry(channel_id).or_insert_with(|| Channel {
            channel_id,
            parent_id: None,
            name: String::new(),
            description: String::new(),
            position: 0,
            temporary: false,
            max_users: 0,
        });

        if let Some(parent) = state.parent {
            channel.parent_id = Some(parent);
        }
        if let Some(ref name) = state.name {
            channel.name = name.clone();
        }
        if let Some(ref desc) = state.description {
            channel.description = desc.clone();
        }
        if let Some(pos) = state.position {
            channel.position = pos;
        }
        if let Some(temp) = state.temporary {
            channel.temporary = temp;
        }
        if let Some(max) = state.max_users {
            channel.max_users = max;
        }
    }

    /// Remove a channel from state.
    pub fn remove_channel(&mut self, channel_id: u32) {
        self.channels.remove(&channel_id);
    }

    /// Apply `ServerSync` to record our session and connection metadata.
    pub fn apply_server_sync(&mut self, sync: &crate::proto::mumble_tcp::ServerSync) {
        self.connection.session_id = sync.session;
        self.connection.max_bandwidth = sync.max_bandwidth;
        self.connection.welcome_text = sync.welcome_text.clone();
    }

    /// Get our own session ID.
    pub fn own_session(&self) -> Option<u32> {
        self.connection.session_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::mumble_tcp;

    #[test]
    fn new_state_is_empty() {
        let state = ServerState::new();
        assert!(state.users.is_empty());
        assert!(state.channels.is_empty());
        assert!(state.own_session().is_none());
        assert!(state.connection.welcome_text.is_none());
    }

    #[test]
    fn apply_user_state_creates_user() {
        let mut state = ServerState::new();
        let user_state = mumble_tcp::UserState {
            session: Some(1),
            name: Some("Alice".into()),
            channel_id: Some(0),
            ..Default::default()
        };
        state.apply_user_state(&user_state);
        assert_eq!(state.users.len(), 1);
        let user = &state.users[&1];
        assert_eq!(user.name, "Alice");
        assert_eq!(user.channel_id, 0);
        assert_eq!(user.session, 1);
    }

    #[test]
    fn apply_user_state_updates_existing_user() {
        let mut state = ServerState::new();
        let create = mumble_tcp::UserState {
            session: Some(1),
            name: Some("Alice".into()),
            channel_id: Some(0),
            ..Default::default()
        };
        state.apply_user_state(&create);

        // Partial update - only change channel
        let update = mumble_tcp::UserState {
            session: Some(1),
            channel_id: Some(5),
            ..Default::default()
        };
        state.apply_user_state(&update);
        assert_eq!(state.users.len(), 1);
        let user = &state.users[&1];
        assert_eq!(user.name, "Alice"); // unchanged
        assert_eq!(user.channel_id, 5); // updated
    }

    #[test]
    fn apply_user_state_without_session_is_noop() {
        let mut state = ServerState::new();
        let user_state = mumble_tcp::UserState {
            name: Some("Ghost".into()),
            ..Default::default()
        };
        state.apply_user_state(&user_state);
        assert!(state.users.is_empty());
    }

    #[test]
    fn apply_user_state_mute_deaf_flags() {
        let mut state = ServerState::new();
        let user_state = mumble_tcp::UserState {
            session: Some(1),
            mute: Some(true),
            deaf: Some(true),
            self_mute: Some(false),
            self_deaf: Some(false),
            ..Default::default()
        };
        state.apply_user_state(&user_state);
        let user = &state.users[&1];
        assert!(user.mute);
        assert!(user.deaf);
        assert!(!user.self_mute);
        assert!(!user.self_deaf);
    }

    #[test]
    fn remove_user() {
        let mut state = ServerState::new();
        let user_state = mumble_tcp::UserState {
            session: Some(42),
            name: Some("Bob".into()),
            ..Default::default()
        };
        state.apply_user_state(&user_state);
        assert_eq!(state.users.len(), 1);

        state.remove_user(42);
        assert!(state.users.is_empty());
    }

    #[test]
    fn remove_nonexistent_user_is_noop() {
        let mut state = ServerState::new();
        state.remove_user(999);
        assert!(state.users.is_empty());
    }

    #[test]
    fn apply_channel_state_creates_channel() {
        let mut state = ServerState::new();
        let channel_state = mumble_tcp::ChannelState {
            channel_id: Some(0),
            name: Some("Root".into()),
            parent: Some(0),
            position: Some(0),
            ..Default::default()
        };
        state.apply_channel_state(&channel_state);
        assert_eq!(state.channels.len(), 1);
        let ch = &state.channels[&0];
        assert_eq!(ch.name, "Root");
        assert_eq!(ch.parent_id, Some(0));
    }

    #[test]
    fn apply_channel_state_updates_existing() {
        let mut state = ServerState::new();
        let create = mumble_tcp::ChannelState {
            channel_id: Some(1),
            name: Some("Lobby".into()),
            ..Default::default()
        };
        state.apply_channel_state(&create);

        let update = mumble_tcp::ChannelState {
            channel_id: Some(1),
            description: Some("Welcome!".into()),
            temporary: Some(true),
            max_users: Some(10),
            ..Default::default()
        };
        state.apply_channel_state(&update);
        assert_eq!(state.channels.len(), 1);
        let ch = &state.channels[&1];
        assert_eq!(ch.name, "Lobby"); // unchanged
        assert_eq!(ch.description, "Welcome!");
        assert!(ch.temporary);
        assert_eq!(ch.max_users, 10);
    }

    #[test]
    fn apply_channel_state_without_id_is_noop() {
        let mut state = ServerState::new();
        let channel_state = mumble_tcp::ChannelState {
            name: Some("Ghost".into()),
            ..Default::default()
        };
        state.apply_channel_state(&channel_state);
        assert!(state.channels.is_empty());
    }

    #[test]
    fn remove_channel() {
        let mut state = ServerState::new();
        let channel_state = mumble_tcp::ChannelState {
            channel_id: Some(5),
            name: Some("AFK".into()),
            ..Default::default()
        };
        state.apply_channel_state(&channel_state);
        assert_eq!(state.channels.len(), 1);

        state.remove_channel(5);
        assert!(state.channels.is_empty());
    }

    #[test]
    fn apply_server_sync() {
        let mut state = ServerState::new();
        let sync = mumble_tcp::ServerSync {
            session: Some(7),
            max_bandwidth: Some(72000),
            welcome_text: Some("Hello!".into()),
            ..Default::default()
        };
        state.apply_server_sync(&sync);
        assert_eq!(state.own_session(), Some(7));
        assert_eq!(state.connection.max_bandwidth, Some(72000));
        assert_eq!(state.connection.welcome_text.as_deref(), Some("Hello!"));
    }

    #[test]
    fn multiple_users_and_channels() {
        let mut state = ServerState::new();
        for i in 0..5 {
            state.apply_user_state(&mumble_tcp::UserState {
                session: Some(i),
                name: Some(format!("User{i}")),
                ..Default::default()
            });
            state.apply_channel_state(&mumble_tcp::ChannelState {
                channel_id: Some(i),
                name: Some(format!("Channel{i}")),
                ..Default::default()
            });
        }
        assert_eq!(state.users.len(), 5);
        assert_eq!(state.channels.len(), 5);

        state.remove_user(2);
        state.remove_channel(3);
        assert_eq!(state.users.len(), 4);
        assert_eq!(state.channels.len(), 4);
        assert!(!state.users.contains_key(&2));
        assert!(!state.channels.contains_key(&3));
    }

    #[test]
    fn user_comment_and_hash() {
        let mut state = ServerState::new();
        state.apply_user_state(&mumble_tcp::UserState {
            session: Some(1),
            comment: Some("I'm a bot".into()),
            hash: Some("abc123".into()),
            ..Default::default()
        });
        let user = &state.users[&1];
        assert_eq!(user.comment, "I'm a bot");
        assert_eq!(user.hash, "abc123");
    }
}
