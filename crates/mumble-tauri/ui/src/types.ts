/** Lightweight value types mirroring the Rust backend structs. */

export interface ChannelEntry {
  id: number;
  parent_id: number | null;
  name: string;
  description: string;
  user_count: number;
  /** Server-reported permission bitmask, or null if not yet queried. */
  permissions: number | null;
}

export interface UserEntry {
  session: number;
  name: string;
  channel_id: number;
  /** Raw avatar image bytes (PNG/JPEG), or null if not set. */
  texture: number[] | null;
  /** Mumble comment - may contain FancyMumble profile JSON marker. */
  comment: string | null;
}

export interface ChatMessage {
  sender_session: number | null;
  sender_name: string;
  body: string;
  channel_id: number;
  is_own: boolean;
  /** When set, this message is a DM. Value is the other user's session ID. */
  dm_session?: number | null;
  /** When set, this message belongs to a group chat. Value is the group UUID. */
  group_id?: string | null;
  /** Unique message identifier (Fancy Mumble extension). Absent on legacy servers. */
  message_id?: string | null;
  /** Unix epoch milliseconds (Fancy Mumble extension). Absent on legacy servers. */
  timestamp?: number | null;
}

/** A multi-member group chat, identified by a UUID. */
export interface GroupChat {
  /** Unique group identifier (UUID v4). */
  id: string;
  /** Human-readable group name chosen by the creator. */
  name: string;
  /** Session IDs of all members (including the creator). */
  members: number[];
  /** Session ID of the user who created the group. */
  creator: number;
}

export type ConnectionStatus = "disconnected" | "connecting" | "connected";

export interface MumbleServerConfig {
  max_message_length: number;
  max_image_message_length: number;
  allow_html: boolean;
}

/** Aggregated server info from the backend (version, host, codec, etc.). */
export interface ServerInfo {
  host: string;
  port: number;
  user_count: number;
  max_users: number | null;
  protocol_version: string | null;
  fancy_version: number | null;
  release: string | null;
  os: string | null;
  max_bandwidth: number | null;
  opus: boolean;
}

/** A saved server connection stored persistently. */
export interface SavedServer {
  /** Unique id (crypto.randomUUID). */
  id: string;
  /** Display label chosen by the user - falls back to host. */
  label: string;
  host: string;
  port: number;
  username: string;
  /** TLS client certificate label, or null to connect anonymously. */
  cert_label: string | null;
}

/** Result of pinging a server via TCP. */
export interface ServerPingResult {
  online: boolean;
  /** Round-trip time in ms, null when offline. */
  latency_ms: number | null;
}

// ─── User Preferences ─────────────────────────────────────────────

/** Whether the user prefers a simplified or full-featured UI. */
export type UserMode = "normal" | "expert" | "developer";

/** Preferred time display format. */
export type TimeFormat = "12h" | "24h" | "auto";

/** App-wide user preferences stored persistently. */
export interface UserPreferences {
  /** Simplified or full-featured UI mode. */
  userMode: UserMode;
  /** Whether the first-run setup has been completed. */
  hasCompletedSetup: boolean;
  /** Default username pre-filled when adding a new server. */
  defaultUsername: string;
  /** Custom Klipy API key (expert mode). When empty/undefined, the built-in key is used. */
  klipyApiKey?: string;
  /** Preferred time format for message timestamps. */
  timeFormat: TimeFormat;
  /** Convert UTC timestamps to the local timezone before displaying. */
  convertToLocalTime: boolean;
}

/** Debug statistics returned by the backend for the developer info panel. */
export interface DebugStats {
  channel_message_count: number;
  dm_message_count: number;
  group_message_count: number;
  total_message_count: number;
  offloaded_count: number;
  channel_count: number;
  user_count: number;
  group_count: number;
  connection_epoch: number;
  voice_state: string;
  uptime_seconds: number;
}

// ─── Audio / Voice ────────────────────────────────────────────────

export interface AudioDevice {
  name: string;
  is_default: boolean;
}

export interface AudioSettings {
  /** Selected input device name (null = system default). */
  selected_device: string | null;
  /** Whether auto-gain is enabled. */
  auto_gain: boolean;
  /** Voice activation open threshold (0.0\u20131.0). */
  vad_threshold: number;
  /** AGC max gain boost in dB (expert, default 15). */
  max_gain_db: number;
  /** Close-threshold ratio relative to vad_threshold (expert, default 0.8). */
  noise_gate_close_ratio: number;
  /** Frames to hold the gate open after audio drops below threshold (expert). */
  hold_frames: number;
  /** Use push-to-talk instead of voice activation. */
  push_to_talk: boolean;
  /** Global shortcut string for PTT, e.g. "Alt+T". */
  push_to_talk_key: string | null;  /** Opus encoder bitrate in bits/s (e.g. 72000). */
  bitrate_bps: number;
  /** Audio duration per Opus packet in ms (10, 20, 40, or 60). */
  frame_size_ms: number;
  /** Whether noise suppression (noise gate) is enabled. */
  noise_suppression: boolean;
  /** Selected output device name (null = system default). */
  selected_output_device: string | null;
  /** Microphone volume multiplier (0.0-2.0, default 1.0). */
  input_volume: number;
  /** Speaker volume multiplier (0.0-2.0, default 1.0). */
  output_volume: number;
  /** Automatically adjust VAD threshold based on ambient noise floor. */
  auto_input_sensitivity: boolean;
}

export type VoiceState = "inactive" | "active" | "muted";

// ─── Super Search ─────────────────────────────────────────────────

export type SearchCategory = "channel" | "user" | "group" | "message";

export interface SearchResult {
  category: SearchCategory;
  score: number;
  title: string;
  subtitle: string | null;
  id: number | null;
  string_id: string | null;
}

// ─── FancyMumble Profile ──────────────────────────────────────────

/**
 * Profile customisation data embedded in the Mumble user comment.
 *
 * Everything except the avatar texture is stored here.  Binary values
 * (banner images) are base64 data-URIs because the comment protobuf
 * field is `string` (UTF-8 only).
 */
export interface FancyProfile {
  /** Format version - always `1`. */
  v?: 1;
  /** Avatar frame decoration id. */
  decoration?: string;
  /** Nameplate style id. */
  nameplate?: string;
  /** Animated profile effect id (e.g. "particles", "rain", "pulse_glow"). */
  effect?: string;
  /** Banner configuration. */
  banner?: {
    /** Background colour (hex). */
    color?: string;
    /** Banner image as a data-URI. */
    image?: string;
  };
  /** Name rendering style. */
  nameStyle?: {
    font?: string;
    color?: string;
    gradient?: [string, string];
    glow?: { color: string; size: number };
    bold?: boolean;
    italic?: boolean;
  };
  /** Card background preset id or custom CSS value. */
  cardBackground?: string;
  /** Custom card background (only used when cardBackground is "custom"). */
  cardBackgroundCustom?: string;
  /** Avatar border style preset id. */
  avatarBorder?: string;
  /** Custom avatar border CSS (only used when avatarBorder is "custom"). */
  avatarBorderCustom?: string;
  /** Custom user status text (shown below the name). */
  status?: string;
}
