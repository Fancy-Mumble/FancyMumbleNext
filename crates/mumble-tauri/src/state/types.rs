//! UI value types, event payloads, and configuration structs serialised
//! to the React frontend.

use std::collections::HashMap;

use serde::{Serialize, Serializer};

use mumble_protocol::state::PchatMode;

// --- Serialization helpers ----------------------------------------

fn serialize_pchat_mode<S: Serializer>(mode: &Option<PchatMode>, s: S) -> Result<S::Ok, S::Error> {
    match mode {
        Some(m) => s.serialize_str(match m {
            PchatMode::None => "none",
            PchatMode::PostJoin => "post_join",
            PchatMode::FullArchive => "full_archive",
            PchatMode::ServerManaged => "server_managed",
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
    /// Persistent-chat mode.  `None` if not announced by the server.
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "serialize_pchat_mode")]
    pub pchat_mode: Option<PchatMode>,
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
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
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

/// Payload emitted when a PchatFetchResponse has been fully processed.
#[derive(Clone, Serialize)]
pub(crate) struct PchatFetchCompletePayload {
    pub channel_id: u32,
    pub has_more: bool,
    pub total_stored: u32,
}

// --- Audio types --------------------------------------------------

/// Microphone amplitude payload emitted during mic test.
#[cfg(not(target_os = "android"))]
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
}

// --- Search types -------------------------------------------------

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
    pub auto_input_sensitivity: bool,}

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
    #[cfg(not(target_os = "android"))]
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
