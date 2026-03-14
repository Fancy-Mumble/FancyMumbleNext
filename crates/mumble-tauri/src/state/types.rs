//! UI value types, event payloads, and configuration structs serialised
//! to the React frontend.

use std::collections::HashMap;

use serde::Serialize;

// ─── UI value types (serializable to the frontend) ────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelEntry {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
    pub description: String,
    pub user_count: u32,
    /// Server-reported permission bitmask for this channel.
    /// `None` until a `PermissionQuery` response is received.
    pub permissions: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserEntry {
    pub session: u32,
    pub name: String,
    pub channel_id: u32,
    pub texture: Option<Vec<u8>>,
    pub comment: Option<String>,
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

// ─── Server config ────────────────────────────────────────────────

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

// ─── Group chat ────────────────────────────────────────────────────

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

// ─── Event payloads emitted to the frontend ───────────────────────

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
}

#[derive(Clone, Serialize)]
pub(crate) struct UnreadPayload {
    /// `channel_id` → unread count
    pub unreads: HashMap<u32, u32>,
}

#[derive(Clone, Serialize)]
pub(crate) struct DmUnreadPayload {
    /// `session_id` → unread DM count
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
    /// `group_id` → unread count.
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

// ─── Audio types ──────────────────────────────────────────────────

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
    /// Voice activation threshold (0.0–1.0). Below this level → silence.
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
    /// Microphone volume multiplier (0.0–2.0, default 1.0).
    #[serde(default = "AudioSettings::default_volume")]
    pub input_volume: f32,
    /// Speaker volume multiplier (0.0–2.0, default 1.0).
    #[serde(default = "AudioSettings::default_volume")]
    pub output_volume: f32,
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
