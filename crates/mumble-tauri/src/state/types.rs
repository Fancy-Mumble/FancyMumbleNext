//! UI value types, event payloads, and configuration structs serialised
//! to the React frontend.

use std::collections::HashMap;

use serde::{Serialize, Deserialize, Serializer};

use mumble_protocol::state::PchatProtocol;

// --- Serialization helpers ----------------------------------------

fn serialize_pchat_protocol<S: Serializer>(protocol: &Option<PchatProtocol>, s: S) -> Result<S::Ok, S::Error> {
    match protocol {
        Some(p) => s.serialize_str(match p {
            PchatProtocol::None => "none",
            PchatProtocol::FancyV1PostJoin => "fancy_v1_post_join",
            PchatProtocol::FancyV1FullArchive => "fancy_v1_full_archive",
            PchatProtocol::ServerManaged => "server_managed",
            PchatProtocol::SignalV1 => "signal_v1",
        }),
        _ => s.serialize_none(),
    }
}

// --- UI value types (serializable to the frontend) ----------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelEntry {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
    pub description: String,
    /// SHA-256 hash of the description blob.  Internal tracking only;
    /// not serialised to the frontend.
    #[serde(skip)]
    pub description_hash: Option<Vec<u8>>,
    pub user_count: u32,
    /// Server-reported permission bitmask for this channel.
    /// `None` until a `PermissionQuery` response is received.
    pub permissions: Option<u32>,
    /// Whether the channel is temporary.
    pub temporary: bool,
    /// Channel sort position.
    pub position: i32,
    /// Maximum users allowed (0 = unlimited).
    pub max_users: u32,
    /// Persistent-chat protocol.  `None` if not announced by the server.
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "serialize_pchat_protocol")]
    pub pchat_protocol: Option<PchatProtocol>,
    /// Maximum stored messages (0 = unlimited).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pchat_max_history: Option<u32>,
    /// Auto-delete after N days (0 = forever).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pchat_retention_days: Option<u32>,
    /// Key custodian cert hashes (Section 5.7).
    #[serde(skip)]
    pub pchat_key_custodians: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserEntry {
    pub session: u32,
    pub name: String,
    pub channel_id: u32,
    /// Registered user ID. `None` means the user is not registered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<u32>,
    pub texture: Option<Vec<u8>>,
    pub comment: Option<String>,
    /// Server-side admin mute.
    pub mute: bool,
    /// Server-side admin deafen.
    pub deaf: bool,
    /// Suppressed by the server (e.g. moved to AFK channel).
    pub suppress: bool,
    /// User has self-muted.
    pub self_mute: bool,
    /// User has self-deafened.
    pub self_deaf: bool,
    /// Priority speaker status.
    pub priority_speaker: bool,
    /// TLS certificate hash (hex-encoded SHA-1). Used as stable identity
    /// for persistent chat key management.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Server-advertised client capabilities (see `UserState.ClientFeature`).
    #[serde(skip)]
    pub client_features: Vec<i32>,
}

impl UserEntry {
    /// Returns `true` if this user advertises E2EE persistent chat support.
    pub fn has_pchat_e2ee(&self) -> bool {
        use mumble_protocol::proto::mumble_tcp::user_state::ClientFeature;
        self.client_features
            .contains(&(ClientFeature::FeaturePchatE2ee as i32))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatMessage {
    pub sender_session: Option<u32>,
    pub sender_name: String,
    pub body: String,
    pub channel_id: u32,
    pub is_own: bool,
    /// When set, this message is a direct message (DM) to/from a specific user.
    /// The value is the *other* user's session ID (the conversation partner).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dm_session: Option<u32>,
    /// When set, this message belongs to a group chat.
    /// The value is the group's unique ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// Unique message identifier (Fancy Mumble extension).
    /// `None` when the server/sender does not support extensions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Unix epoch milliseconds (Fancy Mumble extension).
    /// `None` when the server/sender does not support extensions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
    /// `true` when the message came from a legacy (non-E2EE) client on a
    /// pchat-enabled channel and was therefore sent in plaintext.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_legacy: bool,
}

impl ChatMessage {
    /// Ensure the message has a `message_id`, generating a UUID if absent.
    ///
    /// A stable ID is required so the offloading system can refer to the
    /// message across encrypt/store/restore cycles.
    pub fn ensure_id(&mut self) {
        if self.message_id.is_none() {
            self.message_id = Some(uuid::Uuid::new_v4().to_string());
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
}

// --- Server config ------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ServerConfig {
    pub max_message_length: u32,
    pub max_image_message_length: u32,
    pub allow_html: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            // Mumble defaults per the protocol spec.
            max_message_length: 5000,
            max_image_message_length: 131072,
            allow_html: true,
        }
    }
}

/// Version and configuration metadata announced by the server during handshake.
/// Assembled from `Version`, `ServerSync`, and `ServerConfig` messages.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ServerVersionInfo {
    /// Server release string (e.g. "Mumble 1.5.517").
    pub release: Option<String>,
    /// Server operating system (e.g. "Linux", "Windows").
    pub os: Option<String>,
    /// Server OS version string.
    pub os_version: Option<String>,
    /// Legacy protocol version v1 encoding: (major << 16) | (minor << 8) | patch.
    pub version_v1: Option<u32>,
    /// Protocol version v2 encoding.
    pub version_v2: Option<u64>,
    /// Fancy Mumble extension version (None = standard server).
    pub fancy_version: Option<u64>,
}

/// Full server info payload sent to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    /// Host the client connected to.
    pub host: String,
    /// Port the client connected to.
    pub port: u16,
    /// Number of users currently on the server.
    pub user_count: u32,
    /// Maximum users allowed by the server (from `ServerConfig`).
    pub max_users: Option<u32>,
    /// Human-readable protocol version string.
    pub protocol_version: Option<String>,
    /// Fancy Mumble extension version.
    pub fancy_version: Option<u64>,
    /// Server release string.
    pub release: Option<String>,
    /// Server operating system.
    pub os: Option<String>,
    /// Maximum bandwidth allowed by the server (bits/s).
    pub max_bandwidth: Option<u32>,
    /// Whether Opus codec is supported.
    pub opus: bool,
}

// --- Group chat ----------------------------------------------------

/// Debug statistics for the developer info panel.
#[derive(Debug, Clone, Serialize)]
pub struct DebugStats {
    /// Number of channel messages in memory.
    pub channel_message_count: usize,
    /// Number of DM messages in memory.
    pub dm_message_count: usize,
    /// Number of group messages in memory.
    pub group_message_count: usize,
    /// Total messages (channel + DM + group).
    pub total_message_count: usize,
    /// Number of messages currently offloaded to disk.
    pub offloaded_count: usize,
    /// Number of channels known to the client.
    pub channel_count: usize,
    /// Number of users connected to the server.
    pub user_count: usize,
    /// Number of group chats active.
    pub group_count: usize,
    /// Internal connection epoch counter.
    pub connection_epoch: u64,
    /// Current voice state as a string.
    pub voice_state: String,
    /// Seconds since the app was started.
    pub uptime_seconds: u64,
}

/// A multi-member group chat, identified by a UUID.
///
/// Groups are ephemeral (lifetime of the connection).  Membership is
/// propagated via `PluginDataTransmission` with `data_id = "fancy-group"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupChat {
    /// Unique group identifier (UUID v4).
    pub id: String,
    /// Human-readable group name chosen by the creator.
    pub name: String,
    /// Session IDs of all members (including the creator).
    pub members: Vec<u32>,
    /// Session ID of the user who created the group.
    pub creator: u32,
}

// --- Event payloads emitted to the frontend -----------------------

#[derive(Clone, Serialize)]
pub(crate) struct NewMessagePayload {
    pub channel_id: u32,
}

/// Emitted when a new direct message arrives.
#[derive(Clone, Serialize)]
pub(crate) struct NewDmPayload {
    /// Session ID of the conversation partner (the sender for incoming DMs).
    pub session: u32,
}

#[derive(Clone, Serialize)]
pub(crate) struct RejectedPayload {
    pub reason: String,
    /// Protobuf `Reject.RejectType` value, if available.
    /// `3` = `WrongUserPW`, `4` = `WrongServerPW`.
    pub reject_type: Option<i32>,
}

#[derive(Clone, Serialize)]
pub(crate) struct UnreadPayload {
    /// `channel_id` -> unread count
    pub unreads: HashMap<u32, u32>,
}

#[derive(Clone, Serialize)]
pub(crate) struct DmUnreadPayload {
    /// `session_id` -> unread DM count
    pub unreads: HashMap<u32, u32>,
}

/// Emitted when a new group message arrives.
#[derive(Clone, Serialize)]
pub(crate) struct NewGroupMessagePayload {
    /// The group's unique ID.
    pub group_id: String,
}

/// Emitted when group unread counts change.
#[derive(Clone, Serialize)]
pub(crate) struct GroupUnreadPayload {
    /// `group_id` -> unread count.
    pub unreads: HashMap<String, u32>,
}

/// Emitted when a group chat is created or updated.
#[derive(Clone, Serialize)]
pub(crate) struct GroupCreatedPayload {
    pub group: GroupChat,
}

#[derive(Clone, Serialize)]
pub(crate) struct ListenDeniedPayload {
    pub channel_id: u32,
}

#[derive(Clone, Serialize)]
pub(crate) struct PermissionDeniedPayload {
    pub deny_type: Option<i32>,
    pub reason: Option<String>,
}

#[derive(Clone, Serialize)]
pub(crate) struct PluginDataPayload {
    pub sender_session: Option<u32>,
    pub data: Vec<u8>,
    pub data_id: String,
}

#[derive(Clone, Serialize)]
pub(crate) struct CurrentChannelPayload {
    pub channel_id: u32,
}

/// Payload emitted when pchat history loading starts or finishes for a channel.
#[derive(Clone, Serialize)]
pub(crate) struct PchatHistoryLoadingPayload {
    pub channel_id: u32,
    pub loading: bool,
}

/// Payload emitted when a `PchatFetchResponse` has been fully processed.
#[derive(Clone, Serialize)]
pub(crate) struct PchatFetchCompletePayload {
    pub channel_id: u32,
    pub has_more: bool,
    pub total_stored: u32,
}

/// Payload emitted when a `PchatReactionDeliver` is received (single reaction event).
#[derive(Clone, Serialize)]
pub(crate) struct ReactionDeliverPayload {
    pub channel_id: u32,
    pub message_id: String,
    pub emoji: String,
    pub action: String,
    pub sender_hash: String,
    pub sender_name: String,
    pub timestamp: u64,
}

/// A single stored reaction within a `PchatReactionFetchResponse`.
#[derive(Clone, Serialize)]
pub(crate) struct StoredReactionPayload {
    pub message_id: String,
    pub emoji: String,
    pub sender_hash: String,
    pub sender_name: String,
    pub timestamp: u64,
}

/// Payload emitted when a `PchatReactionFetchResponse` is received (batch of reactions).
#[derive(Clone, Serialize)]
pub(crate) struct ReactionFetchResponsePayload {
    pub channel_id: u32,
    pub reactions: Vec<StoredReactionPayload>,
}

/// A pending key-share request waiting for user approval.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct PendingKeyShare {
    /// Channel that the key would be shared for.
    pub channel_id: u32,
    /// Certificate hash of the peer requesting the key.
    pub peer_cert_hash: String,
    /// Display name of the peer (resolved from current users).
    pub peer_name: String,
    /// Server-assigned request ID (present for consensus key-request path,
    /// `None` for proactive key-announce path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Payload for the "pchat-key-share-request" frontend event.
#[derive(Clone, Serialize)]
pub(crate) struct KeyShareRequestPayload {
    pub channel_id: u32,
    pub peer_name: String,
    pub peer_cert_hash: String,
}
/// Payload for the \"pchat-key-share-requests-changed\" event (after approve/dismiss).
#[derive(Clone, Serialize)]
pub(crate) struct KeyShareRequestsChangedPayload {
    pub channel_id: u32,
    pub pending: Vec<PendingKeyShare>,
}
/// A user known to hold the encryption key for a channel.
#[derive(Clone, Debug, Serialize)]
pub struct KeyHolderEntry {
    /// TLS certificate hash (stable identity).
    pub cert_hash: String,
    /// Display name (resolved from online users or last known).
    pub name: String,
    /// Whether the user is currently online.
    pub is_online: bool,
}

/// Payload for the "pchat-key-holders-changed" event.
#[derive(Clone, Serialize)]
pub(crate) struct KeyHoldersChangedPayload {
    pub channel_id: u32,
    pub holders: Vec<KeyHolderEntry>,
}

/// Payload for the "pchat-key-revoked" event.
#[derive(Clone, Serialize)]
pub(crate) struct PchatKeyRevokedPayload {
    pub channel_id: u32,
}// --- Audio types --------------------------------------------------

/// Microphone amplitude payload emitted during mic test.
#[derive(Clone, Serialize)]
pub(crate) struct MicAmplitudePayload {
    /// RMS amplitude (0.0 - 1.0).
    pub rms: f32,
    /// Peak amplitude (0.0 - 1.0).
    pub peak: f32,
}

/// Latency measurement payload emitted during latency test.
#[derive(Clone, Serialize)]
pub(crate) struct LatencyPayload {
    /// Round-trip time in milliseconds.
    pub rtt_ms: f64,
}

/// UDP crypto packet counters (good / late / lost / resync).
#[derive(Clone, Default, Serialize)]
pub(crate) struct PacketStats {
    pub good: u32,
    pub late: u32,
    pub lost: u32,
    pub resync: u32,
}

/// Rolling-window packet statistics.
#[derive(Clone, Serialize)]
pub(crate) struct RollingStatsPayload {
    /// Rolling window duration in seconds.
    pub time_window: u32,
    pub from_client: PacketStats,
    pub from_server: PacketStats,
}

/// Payload emitted when a `UserStats` response arrives from the server.
#[derive(Clone, Serialize)]
pub(crate) struct UserStatsPayload {
    pub session: u32,
    pub tcp_packets: u32,
    pub udp_packets: u32,
    pub tcp_ping_avg: f32,
    pub tcp_ping_var: f32,
    pub udp_ping_avg: f32,
    pub udp_ping_var: f32,
    pub bandwidth: Option<u32>,
    pub onlinesecs: Option<u32>,
    pub idlesecs: Option<u32>,
    pub strong_certificate: bool,
    pub opus: bool,
    /// Client version string (e.g. "1.5.517").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Operating system name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    /// Operating system version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    /// Client IP address (formatted string).  Only present for admins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// Total UDP crypto stats: packets received from the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_client: Option<PacketStats>,
    /// Total UDP crypto stats: packets sent to the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_server: Option<PacketStats>,
    /// Rolling-window packet statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rolling_stats: Option<RollingStatsPayload>,
}

/// Result sent through the oneshot channel when a `PchatAck` for a deletion
/// request is received from the server.
pub(crate) struct DeleteAckResult {
    pub success: bool,
    pub reason: Option<String>,
}

// --- Admin panel payload types ------------------------------------

/// A registered user entry returned by the server's `UserList` message.
#[derive(Debug, Clone, Serialize)]
pub struct RegisteredUserPayload {
    pub user_id: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_channel: Option<u32>,
}

/// A registered user update sent from the frontend.
///
/// - `name: Some(new_name)` renames the user.
/// - `name: None` deletes (deregisters) the user.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RegisteredUserUpdate {
    pub user_id: u32,
    pub name: Option<String>,
}

/// A ban list entry sent to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct BanEntryPayload {
    pub address: String,
    pub mask: u32,
    pub name: String,
    pub hash: String,
    pub reason: String,
    pub start: String,
    pub duration: u32,
}

/// Full ACL data for a channel, emitted as event payload.
#[derive(Debug, Clone, Serialize)]
pub struct AclPayload {
    pub channel_id: u32,
    pub inherit_acls: bool,
    pub groups: Vec<AclGroupPayload>,
    pub acls: Vec<AclEntryPayload>,
}

/// A channel group entry within an ACL.
#[derive(Debug, Clone, Serialize)]
pub struct AclGroupPayload {
    pub name: String,
    pub inherited: bool,
    pub inherit: bool,
    pub inheritable: bool,
    pub add: Vec<u32>,
    pub remove: Vec<u32>,
    pub inherited_members: Vec<u32>,
}

/// A single ACL rule within a channel's ACL list.
#[derive(Debug, Clone, Serialize)]
pub struct AclEntryPayload {
    pub apply_here: bool,
    pub apply_subs: bool,
    pub inherited: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub grant: u32,
    pub deny: u32,
}

// --- Admin panel input types (deserialized from frontend) ---------

/// A ban entry received from the frontend for updating the ban list.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BanEntryInput {
    pub address: String,
    pub mask: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub start: String,
    #[serde(default)]
    pub duration: u32,
}

/// ACL update payload received from the frontend.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AclInput {
    pub channel_id: u32,
    pub inherit_acls: bool,
    pub groups: Vec<AclGroupInput>,
    pub acls: Vec<AclEntryInput>,
}

/// A group entry from the frontend for ACL updates.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AclGroupInput {
    pub name: String,
    #[serde(default = "default_true")]
    pub inherited: bool,
    #[serde(default = "default_true")]
    pub inherit: bool,
    #[serde(default = "default_true")]
    pub inheritable: bool,
    #[serde(default)]
    pub add: Vec<u32>,
    #[serde(default)]
    pub remove: Vec<u32>,
    #[serde(default)]
    pub inherited_members: Vec<u32>,
}

/// An ACL entry from the frontend for ACL updates.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AclEntryInput {
    #[serde(default = "default_true")]
    pub apply_here: bool,
    #[serde(default = "default_true")]
    pub apply_subs: bool,
    #[serde(default)]
    pub inherited: bool,
    pub user_id: Option<u32>,
    pub group: Option<String>,
    #[serde(default)]
    pub grant: u32,
    #[serde(default)]
    pub deny: u32,
}

const fn default_true() -> bool {
    true
}

// --- Search types -------------------------------------------------

/// Filter narrowing the search scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchFilter {
    All,
    Messages,
    Photos,
    Users,
    Links,
}

/// Category tag for a search result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchCategory {
    Channel,
    User,
    Group,
    Message,
}

/// A single search result returned by the super-search command.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    /// What kind of item this is.
    pub category: SearchCategory,
    /// Fuzzy match score (lower = better match, 0 = exact).
    pub score: u32,
    /// Primary display text (channel name, username, group name, or message snippet).
    pub title: String,
    /// Secondary context (e.g. channel name for a user, sender for a message).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Numeric ID for channels (`channel_id`) or users (session).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
    /// String ID for groups (group UUID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_id: Option<String>,
}

/// A single photo extracted from a chat message for the photo grid.
#[derive(Debug, Clone, Serialize)]
pub struct PhotoEntry {
    /// Image source (data-URL or remote URL).
    pub src: String,
    /// Who sent the message containing this image.
    pub sender_name: String,
    /// Channel ID when the photo is from a channel message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<u32>,
    /// Group ID when the photo is from a group message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// DM session when the photo is from a direct message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dm_session: Option<u32>,
    /// Human-readable context (e.g. "in #General", "DM with Alice").
    pub context: String,
    /// Message timestamp (epoch ms), if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
}

// --- Audio device type --------------------------------------------

/// An available audio input device.
#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    pub name: String,
    pub is_default: bool,
}

/// User-configurable audio settings.
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
pub struct AudioSettings {
    /// Selected input device name (None = system default).
    pub selected_device: Option<String>,
    /// Whether auto-gain is enabled.
    pub auto_gain: bool,
    /// Voice activation threshold (0.0-1.0). Below this level -> silence.
    pub vad_threshold: f32,
    /// AGC maximum gain boost in dB (expert, default 15.0).
    #[serde(default = "AudioSettings::default_max_gain")]
    pub max_gain_db: f32,
    /// Close-threshold ratio relative to `vad_threshold` (expert, default 0.8).
    #[serde(default = "AudioSettings::default_close_ratio")]
    pub noise_gate_close_ratio: f32,
    /// Number of frames to hold the gate open after audio drops below threshold.
    #[serde(default = "AudioSettings::default_hold_frames")]
    pub hold_frames: u32,
    /// Use push-to-talk instead of voice activation.
    #[serde(default)]
    pub push_to_talk: bool,
    /// Global shortcut string for PTT (e.g. "Alt+T").
    #[serde(default)]
    pub push_to_talk_key: Option<String>,
    /// Opus encoder bitrate in bits/s (e.g. 72000).
    #[serde(default = "AudioSettings::default_bitrate")]
    pub bitrate_bps: i32,
    /// Audio duration per Opus packet in milliseconds (10, 20, 40, or 60).
    #[serde(default = "AudioSettings::default_frame_size_ms")]
    pub frame_size_ms: u32,
    /// Whether the noise gate (noise suppression) is enabled.
    #[serde(default = "AudioSettings::default_noise_suppression")]
    pub noise_suppression: bool,
    /// Selected output device name (None = system default).
    #[serde(default)]
    pub selected_output_device: Option<String>,
    /// Microphone volume multiplier (0.0-2.0, default 1.0).
    #[serde(default = "AudioSettings::default_volume")]
    pub input_volume: f32,
    /// Speaker volume multiplier (0.0-2.0, default 1.0).
    #[serde(default = "AudioSettings::default_volume")]
    pub output_volume: f32,    /// Automatically adjust input sensitivity based on ambient noise floor.
    #[serde(default)]
    pub auto_input_sensitivity: bool,
    /// Force audio to use TCP tunnel instead of UDP (e.g. behind strict NAT).
    #[serde(default)]
    pub force_tcp_audio: bool,
}

impl AudioSettings {
    pub(crate) fn default_max_gain() -> f32 {
        15.0
    }
    pub(crate) fn default_close_ratio() -> f32 {
        0.8
    }
    pub(crate) fn default_hold_frames() -> u32 {
        15
    }
    pub(crate) fn default_bitrate() -> i32 {
        72_000
    }
    pub(crate) fn default_frame_size_ms() -> u32 {
        20
    }
    pub(crate) fn default_noise_suppression() -> bool {
        true
    }
    pub(crate) fn default_volume() -> f32 {
        1.0
    }

    /// Convert `frame_size_ms` to samples-per-channel at 48 kHz.
    ///
    /// Clamps to valid Opus frame sizes (10, 20, 40, 60 ms).
    pub fn frame_size_samples(&self) -> usize {
        match self.frame_size_ms {
            10 => 480,
            40 => 1920,
            60 => 2880,
            _ => 960, // 20 ms default
        }
    }

    /// Whether any pipeline-relevant setting differs from `other`.
    ///
    /// PTT key and UI-only fields are excluded since they don't
    /// require a pipeline restart.
    pub fn needs_pipeline_restart(&self, other: &Self) -> bool {
        self.selected_device != other.selected_device
            || self.auto_gain != other.auto_gain
            || (self.vad_threshold - other.vad_threshold).abs() > f32::EPSILON
            || (self.max_gain_db - other.max_gain_db).abs() > f32::EPSILON
            || (self.noise_gate_close_ratio - other.noise_gate_close_ratio).abs() > f32::EPSILON
            || self.hold_frames != other.hold_frames
            || self.bitrate_bps != other.bitrate_bps
            || self.frame_size_ms != other.frame_size_ms
            || self.noise_suppression != other.noise_suppression
            || self.auto_input_sensitivity != other.auto_input_sensitivity
    }

    /// Whether the output device changed, requiring inbound pipeline restart.
    pub fn needs_inbound_restart(&self, other: &Self) -> bool {
        self.selected_output_device != other.selected_output_device
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            selected_device: None,
            auto_gain: true,
            vad_threshold: 0.01,
            max_gain_db: 15.0,
            noise_gate_close_ratio: 0.8,
            hold_frames: 15,
            push_to_talk: false,
            push_to_talk_key: None,
            bitrate_bps: 72_000,
            frame_size_ms: 20,
            noise_suppression: true,
            selected_output_device: None,
            input_volume: 1.0,
            output_volume: 1.0,
            auto_input_sensitivity: false,
            force_tcp_audio: false,
        }
    }
}

/// Current voice state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(target_os = "android", allow(dead_code))]
pub enum VoiceState {
    /// User is deaf + muted (default on connect / before enabling voice).
    #[default]
    Inactive,
    /// User has enabled voice calling - can speak and hear.
    Active,
    /// User is muted (mic off) but can still hear others.
    Muted,
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "test code: panicking on failure is the intended behaviour")]
mod tests {
    use super::*;

    /// Regression test: the frontend sends `"fancy_v1_full_archive"` etc.
    /// and the parser must accept those exact strings.
    #[test]
    fn parse_pchat_protocol_str_roundtrip() {
        use super::super::parse_pchat_protocol_str;

        // Every variant the UI sends must survive a serialize -> parse roundtrip.
        let cases = [
            (PchatProtocol::None, "none"),
            (PchatProtocol::FancyV1PostJoin, "fancy_v1_post_join"),
            (PchatProtocol::FancyV1FullArchive, "fancy_v1_full_archive"),
            (PchatProtocol::ServerManaged, "server_managed"),
            (PchatProtocol::SignalV1, "signal_v1"),
        ];
        for (expected, input) in cases {
            assert_eq!(
                parse_pchat_protocol_str(input),
                expected,
                "parse_pchat_protocol_str({input:?}) should return {expected:?}",
            );
        }
    }

    #[test]
    fn serialize_channel_entry_with_signal_v1() {
        let entry = ChannelEntry {
            id: 5,
            parent_id: Some(0),
            name: "Secret".into(),
            description: String::new(),
            description_hash: None,
            user_count: 2,
            permissions: None,
            temporary: false,
            position: 0,
            max_users: 0,
            pchat_protocol: Some(PchatProtocol::SignalV1),
            pchat_max_history: Some(1000),
            pchat_retention_days: Some(7),
            pchat_key_custodians: Vec::new(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(
            json.contains(r#""pchat_protocol":"signal_v1""#),
            "expected signal_v1 in JSON: {json}",
        );
    }
}
