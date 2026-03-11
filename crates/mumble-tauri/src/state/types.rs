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

// ─── Event payloads emitted to the frontend ───────────────────────

#[derive(Clone, Serialize)]
pub(crate) struct NewMessagePayload {
    pub channel_id: u32,
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
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
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
        }
    }
}

/// Current voice state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VoiceState {
    /// User is deaf + muted (default on connect / before enabling voice).
    #[default]
    Inactive,
    /// User has enabled voice calling - can speak and hear.
    Active,
    /// User is muted (mic off) but can still hear others.
    Muted,
}
