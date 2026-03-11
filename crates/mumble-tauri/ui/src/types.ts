/** Lightweight value types mirroring the Rust backend structs. */

export interface ChannelEntry {
  id: number;
  parent_id: number | null;
  name: string;
  description: string;
  user_count: number;
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
}

export type ConnectionStatus = "disconnected" | "connecting" | "connected";

export interface MumbleServerConfig {
  max_message_length: number;
  max_image_message_length: number;
  allow_html: boolean;
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
export type UserMode = "normal" | "expert";

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
  push_to_talk_key: string | null;
}

export type VoiceState = "inactive" | "active" | "muted";

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
