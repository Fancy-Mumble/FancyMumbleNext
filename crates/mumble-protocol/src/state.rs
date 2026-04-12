//! Tracked server state - users, channels, and connection metadata.
//!
//! The client orchestrator updates this state as server messages arrive.
//! External consumers can query it through the public API.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};


/// Unified pchat protocol indicator for a channel.
///
/// Each value identifies both the E2EE protocol implementation and
/// the persistence behaviour. Maps 1:1 to the `PchatProtocol`
/// protobuf enum. Lives in core (no feature gate) so all consumers
/// can use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "persistent-chat", derive(serde::Serialize, serde::Deserialize))]
pub enum PchatProtocol {
    /// No persistence.  Standard volatile Mumble chat.
    #[default]
    None,
    /// `FancyV1` E2EE with full-archive access (shared archive key).
    FancyV1FullArchive,
    /// Signal Protocol E2EE (Double Ratchet / Sender Keys via libsignal).
    /// Post-join visibility with per-sender forward secrecy.
    SignalV1,
}

impl PchatProtocol {
    /// Parse from the protobuf `PchatProtocol` i32 value.
    #[must_use]
    pub fn from_proto(value: i32) -> Self {
        match value {
            2 => Self::FancyV1FullArchive,
            4 => Self::SignalV1,
            _ => Self::None,
        }
    }

    /// Convert to the protobuf `PchatProtocol` i32 value.
    #[must_use]
    pub fn to_proto(self) -> i32 {
        match self {
            Self::None => 0,
            Self::FancyV1FullArchive => 2,
            Self::SignalV1 => 4,
        }
    }

    /// Whether this protocol uses post-join epoch key semantics.
    #[must_use]
    pub fn is_post_join(&self) -> bool {
        matches!(self, Self::SignalV1)
    }

    /// Whether this protocol uses full-archive shared key semantics.
    #[must_use]
    pub fn is_full_archive(&self) -> bool {
        matches!(self, Self::FancyV1FullArchive)
    }

    /// Whether this protocol uses client-side E2E encryption.
    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        matches!(self, Self::FancyV1FullArchive | Self::SignalV1)
    }

    /// The E2EE algorithm version byte, or `None` if the protocol
    /// does not use client-side encryption.
    #[must_use]
    pub fn protocol_version(&self) -> Option<u8> {
        match self {
            Self::FancyV1FullArchive => Some(1),
            Self::SignalV1 => Some(2),
            Self::None => None,
        }
    }
}

impl fmt::Display for PchatProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::FancyV1FullArchive => write!(f, "FancyV1FullArchive"),
            Self::SignalV1 => write!(f, "SignalV1"),
        }
    }
}

// Re-export version utilities from fancy-utils so existing
// `mumble_protocol::state::fancy_version_*` paths keep working.
pub use fancy_utils::version::{fancy_version_decode, fancy_version_encode, fancy_version_string};


/// Snapshot of a connected user.
#[derive(Debug, Clone)]
pub struct User {
    /// The session ID assigned by the server to this user.
    pub session: u32,
    /// Display name.
    pub name: String,
    /// ID of the channel the user is currently in.
    pub channel_id: u32,
    /// Registered user ID. `None` means the user is not registered.
    pub user_id: Option<u32>,
    /// Whether this user has been server-muted.
    pub mute: bool,
    /// Whether this user has been server-deafened.
    pub deaf: bool,
    /// Whether this user has self-muted.
    pub self_mute: bool,
    /// Whether this user has self-deafened.
    pub self_deaf: bool,
    /// The user's comment (may be HTML or a Fancy Mumble profile blob).
    pub comment: String,
    /// The user's avatar texture bytes (typically PNG/JPEG).
    pub texture: Vec<u8>,
    /// SHA-256 hash of the user's certificate.
    pub hash: String,
}

/// Running TCP ping statistics tracked during the connection.
///
/// Updated every time the server echoes back a `Ping` message containing
/// the timestamp we originally sent.  The accumulated counters are
/// included in subsequent outbound `Ping` messages so the server (and
/// other clients requesting our stats) can see our link quality.
#[derive(Debug, Clone, Default)]
pub struct PingStats {
    /// Number of TCP pings sent.
    pub tcp_packets: u32,
    /// Running average TCP round-trip time in milliseconds.
    pub tcp_ping_avg: f32,
    /// Running variance of TCP round-trip time (ms^2).
    pub tcp_ping_var: f32,
    /// Number of UDP pings sent (placeholder - not yet implemented).
    pub udp_packets: u32,
    /// Running average UDP round-trip time in milliseconds.
    pub udp_ping_avg: f32,
    /// Running variance of UDP round-trip time (ms^2).
    pub udp_ping_var: f32,
    /// Count used internally for incremental variance (Welford's algorithm).
    count: u32,
}

impl PingStats {
    /// Record a new TCP RTT sample using Welford's online algorithm
    /// for numerically stable mean and variance.
    pub fn record_tcp_rtt(&mut self, rtt_ms: f32) {
        self.tcp_packets += 1;
        self.count += 1;
        let n = self.count as f32;
        let delta = rtt_ms - self.tcp_ping_avg;
        self.tcp_ping_avg += delta / n;
        let delta2 = rtt_ms - self.tcp_ping_avg;
        self.tcp_ping_var += (delta * delta2 - self.tcp_ping_var) / n;
    }
}

/// Thread-safe handle to the shared ping statistics.
///
/// Cloned into the periodic ping task so it can read the latest stats
/// while the main event loop updates them.
pub type SharedPingStats = Arc<Mutex<PingStats>>;

/// Snapshot of a channel on the server.
#[derive(Debug, Clone)]
pub struct Channel {
    /// The channel's unique identifier.
    pub channel_id: u32,
    /// Parent channel ID (`None` for the root channel).
    pub parent_id: Option<u32>,
    /// Display name of the channel.
    pub name: String,
    /// Channel description (may contain HTML).
    pub description: String,
    /// SHA-256 hash of the description blob.  When the server sends
    /// only the hash (no inline `description`), the client must
    /// request the full blob via `RequestBlob::channel_description`.
    pub description_hash: Option<Vec<u8>>,
    /// Display order hint relative to sibling channels.
    pub position: i32,
    /// Whether the channel is temporary (auto-deleted when empty).
    pub temporary: bool,
    /// Maximum number of users allowed (0 = unlimited).
    pub max_users: u32,
    /// Server-reported permission bitmask for this channel.
    /// `None` until a `PermissionQuery` response is received.
    pub permissions: Option<u32>,
    /// Whether the server reports the channel requires special
    /// permissions to enter (`ChannelState.is_enter_restricted`).
    pub is_enter_restricted: bool,
    /// Whether the server reports the current user can enter
    /// this channel (`ChannelState.can_enter`).
    pub can_enter: bool,
    /// Persistent-chat protocol.  `None` if not announced by the server.
    pub pchat_protocol: Option<PchatProtocol>,
    /// Maximum stored messages (0 = unlimited).  `None` if not set.
    pub pchat_max_history: Option<u32>,
    /// Auto-delete after N days (0 = forever).  `None` if not set.
    pub pchat_retention_days: Option<u32>,
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
    /// The Fancy Mumble version announced by the server (if any).
    /// `None` means the server is a standard Mumble server without
    /// Fancy Mumble extensions.
    pub server_fancy_version: Option<u64>,
}

/// Aggregated server state maintained by the client.
#[derive(Debug, Default)]
pub struct ServerState {
    /// Connection-level metadata (session ID, welcome text, etc.).
    pub connection: ConnectionInfo,
    /// All currently connected users, keyed by session ID.
    pub users: HashMap<u32, User>,
    /// All known channels, keyed by channel ID.
    pub channels: HashMap<u32, Channel>,
    /// Shared ping statistics - updated on every Ping echo from the server,
    /// read by the periodic ping task to populate outbound Ping messages.
    pub ping_stats: SharedPingStats,
}

impl ServerState {
    /// Create a new, empty server state.
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
            user_id: None,
            mute: false,
            deaf: false,
            self_mute: false,
            self_deaf: false,
            comment: String::new(),
            texture: Vec::new(),
            hash: String::new(),
        });

        let _ = state.name.as_ref().inspect(|v| user.name.clone_from(v));
        let _ = state.channel_id.inspect(|&v| user.channel_id = v);
        let _ = state.mute.inspect(|&v| user.mute = v);
        let _ = state.deaf.inspect(|&v| user.deaf = v);
        let _ = state.self_mute.inspect(|&v| user.self_mute = v);
        let _ = state.self_deaf.inspect(|&v| user.self_deaf = v);
        let _ = state.comment.as_ref().inspect(|v| user.comment.clone_from(v));
        let _ = state.texture.as_ref().inspect(|v| user.texture.clone_from(v));
        let _ = state.hash.as_ref().inspect(|v| user.hash.clone_from(v));
        let _ = state.user_id.inspect(|&v| user.user_id = Some(v));
    }

    /// Remove a user from state.
    pub fn remove_user(&mut self, session: u32) {
        let _ = self.users.remove(&session);
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
            description_hash: None,
            position: 0,
            temporary: false,
            max_users: 0,
            permissions: None,
            is_enter_restricted: false,
            can_enter: true,
            pchat_protocol: None,
            pchat_max_history: None,
            pchat_retention_days: None,
        });

        let _ = state.parent.inspect(|&v| channel.parent_id = Some(v));
        let _ = state.name.as_ref().inspect(|v| channel.name.clone_from(v));
        let _ = state.description.as_ref().inspect(|v| channel.description.clone_from(v));
        let _ = state.description_hash.as_ref().inspect(|v| channel.description_hash = Some((*v).clone()));
        let _ = state.position.inspect(|&v| channel.position = v);
        let _ = state.temporary.inspect(|&v| channel.temporary = v);
        let _ = state.max_users.inspect(|&v| channel.max_users = v);
        let _ = state.is_enter_restricted.inspect(|&v| channel.is_enter_restricted = v);
        let _ = state.can_enter.inspect(|&v| channel.can_enter = v);
        let _ = state.pchat_protocol.inspect(|&v| channel.pchat_protocol = Some(PchatProtocol::from_proto(v)));
        let _ = state.pchat_max_history.inspect(|&v| channel.pchat_max_history = Some(v));
        let _ = state.pchat_retention_days.inspect(|&v| channel.pchat_retention_days = Some(v));
    }

    /// Apply a `PermissionQuery` response from the server.
    ///
    /// If `flush` is set, all cached permissions are cleared first.
    pub fn apply_permission_query(&mut self, pq: &crate::proto::mumble_tcp::PermissionQuery) {
        if pq.flush() {
            for ch in self.channels.values_mut() {
                ch.permissions = None;
            }
        }

        if let (Some(channel_id), Some(perms)) = (pq.channel_id, pq.permissions) {
            if let Some(ch) = self.channels.get_mut(&channel_id) {
                ch.permissions = Some(perms);
            }
        }
    }

    /// Remove a channel from state.
    pub fn remove_channel(&mut self, channel_id: u32) {
        let _ = self.channels.remove(&channel_id);
    }

    /// Apply an incoming `Version` message from the server.
    ///
    /// If the server sends `fancy_version`, it supports Fancy Mumble
    /// extensions (`message_id`, `timestamp`, etc.).
    pub fn apply_version(&mut self, version: &crate::proto::mumble_tcp::Version) {
        self.connection.server_fancy_version = version.fancy_version;
    }

    /// Apply `ServerSync` to record our session and connection metadata.
    pub fn apply_server_sync(&mut self, sync: &crate::proto::mumble_tcp::ServerSync) {
        self.connection.session_id = sync.session;
        self.connection.max_bandwidth = sync.max_bandwidth;
        self.connection.welcome_text = sync.welcome_text.clone();

        // `ServerSync.permissions` contains the permission bitmask for the
        // root channel (channel 0).  Store it on the channel if known.
        if let Some(perms) = sync.permissions {
            if let Some(ch) = self.channels.get_mut(&0) {
                ch.permissions = Some(perms as u32);
            }
        }
    }

    /// Get our own session ID.
    pub fn own_session(&self) -> Option<u32> {
        self.connection.session_id
    }

    /// Record a TCP ping round-trip sample.
    ///
    /// Called by the event loop when the server echoes back our `Ping`.
    pub fn record_tcp_ping(&self, rtt_ms: f32) {
        if let Ok(mut stats) = self.ping_stats.lock() {
            stats.record_tcp_rtt(rtt_ms);
        }
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

    #[test]
    fn signal_v1_protocol_roundtrip() {
        let proto_val = PchatProtocol::SignalV1.to_proto();
        assert_eq!(proto_val, 4);
        assert_eq!(PchatProtocol::from_proto(proto_val), PchatProtocol::SignalV1);
    }

    #[test]
    fn signal_v1_is_post_join_and_encrypted() {
        let p = PchatProtocol::SignalV1;
        assert!(p.is_post_join(), "SignalV1 should be post-join");
        assert!(p.is_encrypted(), "SignalV1 should be encrypted");
    }

    #[test]
    fn signal_v1_generation_version() {
        assert_eq!(PchatProtocol::SignalV1.protocol_version(), Some(2));
    }

    #[test]
    fn signal_v1_display() {
        assert_eq!(format!("{}", PchatProtocol::SignalV1), "SignalV1");
    }

    #[test]
    fn channel_state_with_signal_v1_protocol() {
        let mut state = ServerState::new();
        state.apply_channel_state(&mumble_tcp::ChannelState {
            channel_id: Some(10),
            name: Some("Encrypted".into()),
            pchat_protocol: Some(PchatProtocol::SignalV1.to_proto()),
            pchat_max_history: Some(500),
            pchat_retention_days: Some(30),
            ..Default::default()
        });
        let ch = &state.channels[&10];
        assert_eq!(ch.pchat_protocol, Some(PchatProtocol::SignalV1));
        assert_eq!(ch.pchat_max_history, Some(500));
        assert_eq!(ch.pchat_retention_days, Some(30));
    }
}
