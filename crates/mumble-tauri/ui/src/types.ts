/** Lightweight value types mirroring the Rust backend structs. */

/** Persistent-chat mode for a channel. */
export type PchatMode = "none" | "post_join" | "full_archive" | "server_managed";

export interface ChannelEntry {
  id: number;
  parent_id: number | null;
  name: string;
  description: string;
  user_count: number;
  /** Server-reported permission bitmask, or null if not yet queried. */
  permissions: number | null;
  /** Whether the channel is temporary. */
  temporary: boolean;
  /** Channel sort position. */
  position: number;
  /** Maximum users allowed (0 = unlimited). */
  max_users: number;
  /** Persistent-chat mode, if announced by the server. */
  pchat_mode?: PchatMode;
  /** Maximum stored messages (0 = unlimited). */
  pchat_max_history?: number;
  /** Auto-delete after N days (0 = forever). */
  pchat_retention_days?: number;
}

export interface UserEntry {
  session: number;
  name: string;
  channel_id: number;
  /** Registered user ID, or null/undefined if not registered. */
  user_id?: number | null;
  /** Raw avatar image bytes (PNG/JPEG), or null if not set. */
  texture: number[] | null;
  /** Mumble comment - may contain FancyMumble profile JSON marker. */
  comment: string | null;
  /** Server-side admin mute. */
  mute: boolean;
  /** Server-side admin deafen. */
  deaf: boolean;
  /** Suppressed by the server. */
  suppress: boolean;
  /** User has self-muted. */
  self_mute: boolean;
  /** User has self-deafened. */
  self_deaf: boolean;
  /** Priority speaker status. */
  priority_speaker: boolean;
  /** TLS certificate hash (hex-encoded SHA-1). Used as stable identity. */
  hash?: string;
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
  /** When true the message was sent by a legacy (non-E2EE) client on a pchat channel. */
  is_legacy?: boolean;
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

// --- Public Server List -------------------------------------------

/** A public Mumble server from the official directory. */
export interface PublicServer {
  name: string;
  country: string;
  country_code: string;
  ip: string;
  port: number;
  region: string;
  url: string;
}

// --- User Preferences ---------------------------------------------

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
  /** Whether native OS notifications are enabled. */
  enableNotifications?: boolean;
  /** Collapsed/expanded state of sidebar sections. */
  sidebarSections?: SidebarSections;
}

/** Persisted open/closed state for each sidebar section. */
export interface SidebarSections {
  channels: boolean;
  groups: boolean;
  online: boolean;
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

// --- Audio / Voice ------------------------------------------------

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

// --- User Stats (ping statistics) ---------------------------------

/** UDP crypto packet counters. */
export interface PacketStats {
  good: number;
  late: number;
  lost: number;
  resync: number;
}

/** Rolling-window packet statistics. */
export interface RollingStats {
  /** Rolling window duration in seconds. */
  time_window: number;
  from_client: PacketStats;
  from_server: PacketStats;
}

/** Ping and connection statistics for a user, returned by the server. */
export interface UserStats {
  session: number;
  tcp_packets: number;
  udp_packets: number;
  tcp_ping_avg: number;
  tcp_ping_var: number;
  udp_ping_avg: number;
  udp_ping_var: number;
  bandwidth: number | null;
  onlinesecs: number | null;
  idlesecs: number | null;
  strong_certificate: boolean;
  opus: boolean;
  /** Client version string (e.g. "1.5.517"). */
  version?: string | null;
  /** Operating system name. */
  os?: string | null;
  /** Operating system version. */
  os_version?: string | null;
  /** Client IP address (formatted). Only present for admins. */
  address?: string | null;
  /** Total UDP crypto stats: packets received from the client. */
  from_client?: PacketStats | null;
  /** Total UDP crypto stats: packets sent to the client. */
  from_server?: PacketStats | null;
  /** Rolling-window packet statistics. */
  rolling_stats?: RollingStats | null;
}

// --- Super Search -------------------------------------------------

export type SearchCategory = "channel" | "user" | "group" | "message";

export interface SearchResult {
  category: SearchCategory;
  score: number;
  title: string;
  subtitle: string | null;
  id: number | null;
  string_id: string | null;
}

// --- FancyMumble Profile ------------------------------------------

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

// --- Persistent Chat ----------------------------------------------

/** Persistence mode for a channel (maps to Rust PersistenceMode). */
export type PersistenceMode = "NONE" | "POST_JOIN" | "FULL_ARCHIVE";

/** Trust level for a channel's encryption key. */
export type KeyTrustLevel = "ManuallyVerified" | "Verified" | "Unverified" | "Disputed";

/** Persistence configuration for a channel. */
export interface ChannelPersistConfig {
  mode: PersistenceMode;
  maxHistory: number;
  retentionDays: number;
  keyCustodians: string[];
}

/** Per-channel persistence UI state tracked in the Zustand store. */
export interface ChannelPersistenceState {
  mode: PersistenceMode;
  maxHistory: number;
  retentionDays: number;
  hasMore: boolean;
  isFetching: boolean;
  totalStored: number;
}

/** Key trust state for a channel's encryption key. */
export interface KeyTrustState {
  trustLevel: KeyTrustLevel;
  fingerprint: KeyFingerprints;
  distributorName: string;
  distributorHash: string;
  lastChanged: number;
}

/** Fingerprint representations for a channel encryption key. */
export interface KeyFingerprints {
  emoji: string[];
  words: string[];
  hex: string;
}

/** Local custodian pin state persisted per channel. */
export interface CustodianPinState {
  pinned: string[];
  confirmed: boolean;
  pendingUpdate?: string[] | null;
}

/** A conflicting key in a dispute. */
export interface ConflictingKey {
  senderHash: string;
  senderName: string;
  fingerprint: string;
  timestamp: number;
}

/** Pending dispute state for a channel. */
export interface PendingDispute {
  conflictingKeys: ConflictingKey[];
  canResolve: boolean;
  selectedSenderHash?: string;
}

/** A stored persistent message returned from history fetch. */
export interface StoredMessage {
  messageId: string;
  channelId: number;
  timestamp: number;
  senderHash: string;
  senderName: string;
  body: string;
  encrypted: boolean;
  epoch?: number;
  chainIndex?: number;
  replacesId?: string | null;
}

/** Response from fetching persistent message history. */
export interface FetchHistoryResponse {
  channelId: number;
  messages: StoredMessage[];
  hasMore: boolean;
  totalStored: number;
}

/** A pending key-share request waiting for user approval. */
export interface PendingKeyShareRequest {
  channel_id: number;
  peer_cert_hash: string;
  peer_name: string;
}

/** A user known to hold the E2EE key for a channel. */
export interface KeyHolderEntry {
  cert_hash: string;
  name: string;
  is_online: boolean;
}

// --- Admin panel types --------------------------------------------

/** A registered user entry from the server's UserList message. */
export interface RegisteredUser {
  user_id: number;
  name: string;
  last_seen?: string | null;
  last_channel?: number | null;
}

/** Payload for renaming (name set) or deleting (name null) a registered user. */
export interface RegisteredUserUpdate {
  user_id: number;
  name: string | null;
}

/** A ban list entry from the server's BanList message. */
export interface BanEntry {
  address: string;
  mask: number;
  name: string;
  hash: string;
  reason: string;
  start: string;
  duration: number;
}

/** Full ACL data for a channel. */
export interface AclData {
  channel_id: number;
  inherit_acls: boolean;
  groups: AclGroup[];
  acls: AclEntry[];
}

/** A channel group entry within an ACL. */
export interface AclGroup {
  name: string;
  inherited: boolean;
  inherit: boolean;
  inheritable: boolean;
  add: number[];
  remove: number[];
  inherited_members: number[];
}

/** A single ACL rule within a channel's ACL list. */
export interface AclEntry {
  apply_here: boolean;
  apply_subs: boolean;
  inherited: boolean;
  user_id?: number | null;
  group?: string | null;
  grant: number;
  deny: number;
}
