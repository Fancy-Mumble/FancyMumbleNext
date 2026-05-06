/**
 * Global Zustand store for the Mumble Tauri client.
 *
 * All complex logic lives in the Rust backend - the frontend only
 * invokes Tauri commands and reacts to events.
 */

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  isPermissionGranted,
  requestPermission,
  createChannel,
  Importance,
  Visibility,
} from "@tauri-apps/plugin-notification";
import type {
  ChannelEntry,
  UserEntry,
  ChatMessage,
  ConnectionStatus,
  MumbleServerConfig,
  VoiceState,
  PersistenceMode,
  ChannelPersistenceState,
  KeyTrustState,
  CustodianPinState,
  PendingDispute,
  ChannelPersistConfig,
  PchatProtocol,
  PendingKeyShareRequest,
  KeyHolderEntry,
  ServerInfo,
  ServerLogEntry,
  ReadReceiptDeliverPayload,
  FileServerConfig,
  FileServerCapabilities,
  FileAccessMode,
  UploadResponse,
  DownloadEntry,
  CustomServerEmote,
  PendingMessage,
} from "./types";
import type { PollPayload, PollVotePayload } from "./components/chat/PollCreator";
import { registerPoll, registerVote } from "./components/chat/PollCard";
import type { WatchSession, WatchSyncPayload } from "./components/chat/watch/watchTypes";
import { applyWatchSyncEvent } from "./components/chat/watch/watchStore";
import { applyReaction, resetReactions, setServerCustomReactions, type ServerCustomReaction } from "./components/chat/reactionStore";
import { applyReadStates, clearReadReceipts } from "./components/chat/readReceiptStore";
import { offloadManager } from "./messageOffload";
import { getSilencedChannels, setSilencedChannel, getUserVolumes, saveUserVolume, getMutedPushChannels, setMutedPushChannel, getPreferences, updatePreferences } from "./preferencesStorage";
import { loadProfileData } from "./pages/settings/profileData";
import { serializeProfile, dataUrlToBytes } from "./profileFormat";

/** Event payload for a pin state change delivered by the server. */
interface PinDeliverEvent {
  channel_id: number;
  message_id: string;
  pinned: boolean;
  pinner_hash: string;
  pinner_name: string;
  timestamp: number;
}

/** Event payload for a batch of stored pins from the server. */
interface PinFetchResponseEvent {
  channel_id: number;
  pins: {
    message_id: string;
    pinner_hash: string;
    pinner_name: string;
    timestamp: number;
  }[];
}

/** Event payload for a single reaction delivered by the server. */
interface ReactionDeliverEvent {
  channel_id: number;
  message_id: string;
  emoji: string;
  action: string;
  sender_hash: string;
  sender_name: string;
  timestamp: number;
}

/** Event payload for a batch of stored reactions from the server. */
interface ReactionFetchResponseEvent {
  channel_id: number;
  reactions: {
    message_id: string;
    emoji: string;
    sender_hash: string;
    sender_name: string;
    timestamp: number;
  }[];
}

/** Sessions that have already had their stored volume applied this connection. */
const volumeAppliedSessions = new Set<number>();

const AUTO_RECONNECT_DELAY_MS = 3000;

/** Default port the mumble-file-server plugin binds to (see file-server config). */
const DEFAULT_FILE_SERVER_PORT = 64739;

/** Build the file-server REST base URL.
 *
 *  Preference order:
 *  1. `serverConfig.fancy_rest_api_url` (server-side override, used when the
 *     HTTP interface is fronted by a reverse proxy / ingress on a different
 *     hostname than the Mumble TCP port).
 *  2. `http://<connect host>:64739` - the plugin's default loopback bind on
 *     the same hostname the user connected to. */
function fileServerBaseUrl(): string | null {
  const state = useAppStore.getState();
  const override = state.serverConfig.fancy_rest_api_url;
  if (override && override.length > 0) return override.replace(/\/+$/, "");
  const pending = state.pendingConnect;
  if (!pending) return null;
  // Normalize "localhost" to the IPv4 loopback. On Windows, "localhost"
  // resolves to ::1 first, and Docker Desktop's IPv6 port forwarding is
  // unreliable for published ports - the request times out even though
  // `docker port` advertises both 0.0.0.0 and [::] mappings. Forcing
  // IPv4 here avoids the misleading "request failed" probe error during
  // local development.
  const host = pending.host === "localhost" ? "127.0.0.1" : pending.host;
  return `http://${host}:${DEFAULT_FILE_SERVER_PORT}`;
}

/** Rebase a URL returned by the file-server plugin so its origin matches
 *  the current server override URL (e.g. `https://files.mumble.magical.rocks`).
 *  The plugin embeds its own internal origin in download URLs; when the HTTP
 *  interface is fronted by a reverse proxy this origin is wrong for public
 *  access.  Only the scheme + host are replaced; path, query, and fragment
 *  are preserved unchanged.
 *
 *  Returns the original URL unchanged if no override is configured or
 *  parsing fails. */
export function rebaseFileServerUrl(rawUrl: string): string {
  const override = fileServerBaseUrl();
  if (!override) return rawUrl;
  try {
    const u = new URL(rawUrl);
    const o = new URL(override);
    u.protocol = o.protocol;
    u.hostname = o.hostname;
    u.port = o.port;
    return u.toString();
  } catch {
    return rawUrl;
  }
}

/** Probe `GET {baseUrl}/capabilities` and store the result in the Zustand
 *  store.  Called whenever `serverConfig` changes so the Capabilities tab
 *  and Custom-Emotes admin tab populate even when no `fancy-file-server-config`
 *  plugin-data arrives (e.g. when the file-server plugin is loaded but the
 *  user has no upload permissions, or the capabilities endpoint is reachable
 *  via reverse proxy while the per-session config message is suppressed). */
async function probeFileServerCapabilities(): Promise<void> {
  const baseUrl = fileServerBaseUrl();
  if (!baseUrl) return;
  try {
    const body = await invoke<string>("fetch_file_server_capabilities", { baseUrl });
    const caps = JSON.parse(body) as FileServerCapabilities;
    useAppStore.setState({ fileServerCapabilities: caps });
  } catch (e) {
    console.warn("file-server capabilities probe failed:", e);
  }
}

let autoReconnectTimer: ReturnType<typeof setTimeout> | null = null;
let manualDisconnectRequested = false;
/** Sessions whose disconnect was triggered by the user (e.g. via the
 *  tab close button).  The `server-disconnected` listener consults this
 *  set so it does not surface a "Connection lost" overlay for what the
 *  user just initiated themselves.  Entries are removed once handled. */
const intentionallyClosingSessions = new Set<string>();
/** Module-level handle to react-router's `navigate`.  Set by
 *  `initEventListeners`; used by store actions that need to redirect
 *  (e.g. `disconnectSession` falling back to the connect page). */
let navigateRef: ((path: string) => void) | null = null;
let isRestoringVoice = false;

function clearAutoReconnectTimer(): void {
  if (autoReconnectTimer !== null) {
    clearTimeout(autoReconnectTimer);
    autoReconnectTimer = null;
  }
}

async function attemptAutoReconnect(
  fallbackTarget: { host: string; port: number; username: string; certLabel: string | null },
): Promise<void> {
  if (manualDisconnectRequested) return;

  const { getPreferences } = await import("./preferencesStorage");
  const prefs = await getPreferences().catch(() => null);
  if (!prefs?.autoReconnect) return;

  const state = useAppStore.getState();
  if (state.status === "connected" || state.passwordRequired) return;

  const target = state.pendingConnect ?? fallbackTarget;
  await state.connect(target.host, target.port, target.username, target.certLabel ?? null);

  const after = useAppStore.getState();
  if (
    after.status !== "connected"
    && !after.passwordRequired
    && !manualDisconnectRequested
  ) {
    scheduleAutoReconnect(target);
  }
}

function scheduleAutoReconnect(
  fallbackTarget: { host: string; port: number; username: string; certLabel: string | null },
): void {
  clearAutoReconnectTimer();
  autoReconnectTimer = setTimeout(() => {
    void attemptAutoReconnect(fallbackTarget);
  }, AUTO_RECONNECT_DELAY_MS);
}

/**
 * Apply persisted per-user volumes to any users that just appeared.
 * Skips sessions already applied this connection.
 */
function applyStoredVolumesToNewUsers(): void {
  const { users, userVolumes } = useAppStore.getState();
  for (const user of users) {
    if (!user.hash || volumeAppliedSessions.has(user.session)) continue;
    const vol = userVolumes[user.hash];
    if (vol !== undefined && vol !== 100) {
      invoke("set_user_volume", { session: user.session, volume: vol / 100 }).catch(() => {});
    }
    volumeAppliedSessions.add(user.session);
  }
}

// --- Store shape --------------------------------------------------

interface AppState {
  // Reactive state
  status: ConnectionStatus;
  channels: ChannelEntry[];
  users: UserEntry[];
  selectedChannel: number | null;
  /** The channel the user is physically in on the server. */
  currentChannel: number | null;
  /** Session ID of the user whose profile panel is open (right side). */
  selectedUser: number | null;
  /** Our own session ID assigned by the server after connecting. */
  ownSession: number | null;
  messages: ChatMessage[];
  error: string | null;
  listenedChannels: Set<number>;
  unreadCounts: Record<number, number>;
  serverConfig: MumbleServerConfig;
  /** File-server plugin configuration advertised by the server on connect.
   *  `null` when the server has no file-server plugin. */
  fileServerConfig: FileServerConfig | null;
  /** Capabilities fetched from `GET {baseUrl}/capabilities` after receiving
   *  the file-server config. `null` when not yet fetched or no file-server. */
  fileServerCapabilities: FileServerCapabilities | null;
  /** Custom server emotes pushed via `fancy-server-emotes`. Cleared on disconnect. */
  customServerEmotes: CustomServerEmote[];
  /** Locally-saved downloads completed during the current session. Most
   *  recent first. Cleared on disconnect / reset. */
  downloads: DownloadEntry[];
  /** Number of downloads completed since the user last opened the
   *  Downloads panel. Used to drive the kebab-menu badge. */
  unseenDownloadCount: number;
  /** Fancy Mumble version of the connected server (v2-encoded), null if not a fancy server. */
  serverFancyVersion: number | null;
  voiceState: VoiceState;
  /** True when audio is transported over UDP (false = TCP tunnel). */
  udpActive: boolean;
  /** True while the user is in an active mobile call session (set by Start/End Call). */
  inCall: boolean;
  /** Session IDs of users currently transmitting audio (talking). */
  talkingSessions: Set<number>;

  // -- DM state --------------------------------------------------
  /** Session ID of the user whose DM chat is currently viewed. */
  selectedDmUser: number | null;
  /** DM messages for the currently viewed conversation. */
  dmMessages: ChatMessage[];
  /** DM unread counts keyed by user session. */
  dmUnreadCounts: Record<number, number>;

  // -- Poll state (in-memory, not persisted) ---------------------
  /** All known polls keyed by poll ID. */
  polls: Map<string, PollPayload>;
  /** Synthetic local-only messages for rendering polls in the chat flow. */
  pollMessages: ChatMessage[];

  // -- Optimistic outbound messages (in-memory, not persisted) ---
  /** Messages we have started sending but haven't yet confirmed. */
  pendingMessages: PendingMessage[];

  // -- Link embed state (in-memory, not persisted) ---------------
  /** Link embeds keyed by message_id. */
  linkEmbeds: Map<string, import("./types").LinkEmbed[]>;

  /** Whether the user has opted out of requesting link previews. */
  disableLinkPreviews: boolean;

  /** Whether the user allows external embed sources (e.g. YouTube IFrame API). */
  enableExternalEmbeds: boolean;

  /** Streamer mode - when true, sensitive identifiers (host/IP) are
   *  masked across the UI to keep them out of screen captures. */
  streamerMode: boolean;

  /** Monotonic counter incremented whenever the module-level reaction store changes. */
  reactionVersion: number;

  /** Unseen pin message IDs per channel (channel_id -> set of message_ids). */
  unseenPinIds: Map<number, Set<string>>;

  /** Monotonic counter incremented whenever the module-level read receipt store changes. */
  readReceiptVersion: number;

  /** Map of channel_id -> set of session IDs currently typing. */
  typingUsers: Map<number, Set<number>>;

  /** Active watch-together sessions keyed by their session UUID. */
  watchSessions: Map<string, WatchSession>;
  /** Monotonic counter bumped whenever any watch session is mutated. */
  watchSessionsVersion: number;

  // -- Screen share state (in-memory) ----------------------------
  /** Whether we are currently sharing our own screen. */
  isSharingOwn: boolean;
  /** The Mumble session ID of the tab whose webcam/screen is being captured
   *  locally.  Set when `startSharing` succeeds, cleared when broadcasting
   *  stops.  Compared against the current tab's `ownSession` so that other
   *  server tabs in the same window do not mistake themselves for the
   *  broadcaster (which would render a phantom local stream and a stray
   *  "Desktop overlay" button on the wrong tab). */
  broadcastingOwnSession: number | null;
  /** Whether the broadcaster WebRTC connection is still negotiating. */
  webrtcConnecting: boolean;
  /** Inline error message when a WebRTC operation fails (e.g. unreachable SFU). */
  webrtcError: string | null;
  /** Whether the click-through desktop drawing-overlay window is currently
   *  open.  Persisted in the global store (rather than as React local state
   *  in `OwnBroadcastPreview`) so that switching to a different server tab
   *  - which unmounts the preview component - does not implicitly close
   *  the overlay.  Cleared automatically when broadcasting stops. */
  desktopDrawingOverlayOpen: boolean;
  /** Channel IDs where the screen-share drawing tools (color picker,
   *  width slider, clear button) are currently active.  Stored in the
   *  global store so the toggle button in `StreamControls` and the
   *  toolbar in `DrawingOverlay` stay in sync, and so switching tabs
   *  preserves the active state. */
  drawingActiveChannels: Set<number>;
  /** Session IDs of other users currently broadcasting. */
  broadcastingSessions: Set<number>;
  /** Session ID we are currently watching (null if not watching). */
  watchingSession: number | null;
  /** The Mumble session ID of the tab whose viewer is currently watching
   *  `watchingSession`.  Set when `watchBroadcast` runs, cleared when
   *  `stopWatching` runs.  Compared against the current tab's `ownSession`
   *  so that other server tabs in the same window do not mistake the
   *  watch state as their own (which would render a stray RemoteViewer
   *  - including on the broadcaster's tab, where it would try to render
   *  the broadcaster's own session as a remote stream and hang on
   *  "Connecting..."). */
  watchingOwnSession: number | null;

  // -- Persistent chat state -------------------------------------
  /** Persistence metadata per channel (mode, retention, fetch state). */
  channelPersistence: Record<number, ChannelPersistenceState>;
  /** Key trust state per channel (trust level, fingerprints, distributor). */
  keyTrust: Record<number, KeyTrustState>;
  /** Custodian pin state per channel (TOFU pinning). */
  custodianPins: Record<number, CustodianPinState>;
  /** Pending key disputes per channel. */
  pendingDisputes: Record<number, PendingDispute>;
  /** Channels currently loading history (awaiting key exchange + fetch). */
  pchatHistoryLoading: Set<number>;
  /** Pending key-share consent requests per channel. */
  pendingKeyShares: Record<number, PendingKeyShareRequest[]>;
  /** Server-tracked key holders per channel. */
  keyHolders: Record<number, KeyHolderEntry[]>;
  /** Channels where the key-possession challenge failed (key revoked). */
  pchatKeyRevoked: Set<number>;
  /** Error message when the signal bridge library fails to load. */
  signalBridgeError: string | null;

  /** Channel IDs silenced for the current server (notifications suppressed). */
  silencedChannels: Set<number>;

  /** Channel IDs with push notifications disabled (client preference, synced to server). */
  mutedPushChannels: Set<number>;

  /** Channel IDs we are push-subscribed to (have SubscribePush permission). */
  pushSubscribedChannels: Set<number>;

  /** Per-user volume overrides keyed by cert hash (0-200, default 100). */
  /** Per-user volume overrides, keyed by cert hash. */
  userVolumes: Record<string, number>;

  /** Server activity log entries (connect, disconnect, mute, channel move, etc.). */
  serverLog: ServerLogEntry[];

  /** Set when the server rejects with WrongUserPW/WrongServerPW - prompts the UI for a password. */
  passwordRequired: boolean;
  /** True after the user has submitted a password at least once (so we can show rejection errors on retries). */
  passwordAttempted: boolean;
  /** Connection params stored when a password prompt is needed so the user can retry. */
  pendingConnect: { host: string; port: number; username: string; certLabel: string | null } | null;
  /** Certificate label used for the active connection. Stays set until explicit disconnect. */
  connectedCertLabel: string | null;

  /** Human-readable label describing the current post-connect bootstrap
   *  stage ("Negotiating with server...", "Fetching channels...", etc.).
   *  Non-null implies bootstrapping is in progress: the connect-page
   *  loading bar stays visible until this clears, so the chat view is
   *  only revealed once it actually has data.  `null` when idle. */
  bootstrapStage: string | null;

  // Actions
  connect: (host: string, port: number, username: string, certLabel?: string | null, password?: string | null) => Promise<void>;
  disconnect: () => Promise<void>;
  selectChannel: (id: number) => Promise<void>;
  joinChannel: (id: number) => Promise<void>;
  sendMessage: (channelId: number, body: string) => Promise<void>;
  /**
   * Insert a synthetic pending-message placeholder.  Used by the chat
   * composer to surface UI feedback while local media processing
   * (image/video re-encoding) runs BEFORE the actual `send_message`
   * call.  Returns the generated pending id so the caller can dismiss
   * or fail it once processing finishes.
   */
  addPendingPlaceholder: (channelId: number | null, dmSession: number | null, body: string) => string;
  /** Mark an existing pending message as failed with an error message. */
  markPendingFailed: (pendingId: string, errorMessage: string) => void;
  /** Discard a failed pending message. */
  dismissPendingMessage: (pendingId: string) => void;
  /** Retry a failed pending message. */
  retryPendingMessage: (pendingId: string) => Promise<void>;
  editMessage: (channelId: number, messageId: string, newBody: string) => Promise<void>;
  toggleListen: (channelId: number) => Promise<void>;

  // Channel management
  createChannel: (parentId: number, name: string, opts?: {
    description?: string;
    position?: number;
    temporary?: boolean;
    maxUsers?: number;
    pchatProtocol?: PchatProtocol;
    pchatMaxHistory?: number;
    pchatRetentionDays?: number;
  }) => Promise<void>;
  updateChannel: (channelId: number, opts: {
    name?: string;
    description?: string;
    position?: number;
    temporary?: boolean;
    maxUsers?: number;
    pchatProtocol?: PchatProtocol;
    pchatMaxHistory?: number;
    pchatRetentionDays?: number;
  }) => Promise<void>;
  deleteChannel: (channelId: number) => Promise<void>;

  // -- Multi-server (Phase C) ------------------------------------
  /** Snapshot of every backend session currently registered.  Survives
   *  disconnects of individual sessions; only cleared by `refreshSessions`. */
  sessions: import("./types").SessionMeta[];
  /** Backend's currently-active session id (the one frontend commands
   *  without an explicit serverId target).  `null` when no sessions. */
  activeServerId: import("./types").ServerId | null;
  /** Re-pull `list_servers` + `get_active_server` from the backend.
   *  Idempotent; safe to call after any connect / disconnect. */
  refreshSessions: () => Promise<void>;
  /** Make `id` the backend's active session, then refresh per-session
   *  data (channels / users / messages) for the new active session. */
  switchServer: (id: import("./types").ServerId) => Promise<void>;
  /** Tear down a single session by id (used by the tab-close button).
   *  Suppresses the "Connection lost" overlay and switches the active
   *  view to the next remaining session, or to the connect page when
   *  no sessions remain. */
  disconnectSession: (id: import("./types").ServerId) => Promise<void>;
  /** Total unread message count per session (channels + DMs combined),
   *  keyed by serverId.  Updated from `unread-changed` /
   *  `dm-unread-changed` events for non-active sessions; the active
   *  session's totals live in `unreadCounts` / `dmUnreadCounts`. */
  sessionUnreadTotals: Record<string, number>;
  /** Last disconnect / rejection reason per session, keyed by serverId.
   *  Populated by `server-disconnected` / `connection-rejected` listeners
   *  for *every* session (active or not) so that switching to a
   *  disconnected tab restores its specific reason in the UI. */
  sessionErrors: Record<string, string | null>;

  refreshState: () => Promise<void>;
  refreshMessages: (channelId: number) => Promise<void>;
  enableVoice: () => Promise<void>;
  disableVoice: () => Promise<void>;
  toggleMute: () => Promise<void>;
  toggleDeafen: () => Promise<void>;
  selectUser: (session: number | null) => void;
  sendPluginData: (receiverSessions: number[], data: Uint8Array, dataId: string) => Promise<void>;
  /** Upload a local file via the server-side file-server plugin. Returns the
   *  signed download URL on success. Throws if no file-server is configured. */
  uploadFile: (params: {
    filePath: string;
    channelId: number;
    mode: FileAccessMode;
    password?: string;
    filename?: string;
    mimeType?: string;
    uploadId?: string;
  }) => Promise<UploadResponse>;
  /** Download a file (handling password / session-JWT pre-auth automatically)
   *  to `destPath`. Returns the number of bytes written. */
  downloadFile: (params: {
    url: string;
    destPath: string;
    /** Optional password (only used when the file uses `mode=password`). */
    password?: string;
  }) => Promise<number>;
  /** Send a WebRTC screen-sharing signaling message via native proto. */
  sendWebRtcSignal: (targetSession: number, signalType: number, payload: string, serverId?: string | null) => Promise<void>;
  /** Send a reaction (add/remove) on a persistent chat message via native proto. */
  sendReaction: (channelId: number, messageId: string, emoji: string, action: "add" | "remove") => Promise<void>;
  /** Pin or unpin a message in a persistent channel. */
  pinMessage: (channelId: number, messageId: string, unpin: boolean) => Promise<void>;
  /** Mark all unseen pin notifications as seen for a channel. */
  clearUnseenPins: (channelId: number) => void;
  /** Append a completed download to the in-memory list and bump the
   *  unseen badge count. */
  addDownload: (entry: Omit<DownloadEntry, "id" | "downloadedAt">) => void;
  /** Reset the unseen-downloads badge. Called when the panel opens. */
  markDownloadsSeen: () => void;
  /** Remove a single download from the list (does not delete the file). */
  removeDownload: (id: string) => void;
  /** Clear the entire downloads list. */
  clearDownloads: () => void;
  /** Upload a new custom server emote. Requires admin permission. */
  addCustomEmote: (params: {
    shortcode: string;
    aliasEmoji: string;
    description?: string;
    filePath: string;
    mimeType: string;
  }) => Promise<void>;
  /** Delete a custom server emote by shortcode. Requires admin permission. */
  removeCustomEmote: (shortcode: string) => Promise<void>;
  /** Add a poll to the store (called locally when creating a poll). */
  addPoll: (poll: PollPayload, isOwn: boolean) => void;
  setError: (error: string | null) => void;
  reset: () => void;
  /** Retry connection with a password after a WrongUserPW/WrongServerPW rejection. */
  retryWithPassword: (password: string) => Promise<void>;
  /** Dismiss the password prompt without retrying. */
  dismissPasswordPrompt: () => void;

  /** Set a per-user volume override by cert hash (0-200). Persists to disk. */
  setUserVolume: (hash: string, volume: number) => void;

  // Silenced channels
  /** Toggle silence for a channel (local-only, persisted per server). */
  toggleSilenceChannel: (channelId: number) => Promise<boolean>;
  /** Check whether a channel is silenced. */
  isChannelSilenced: (channelId: number) => boolean;

  // Push notification muting
  /** Toggle push-notification mute for a channel (persisted per server, synced to server). */
  toggleMutePushChannel: (channelId: number) => Promise<boolean>;
  /** Check whether push notifications are muted for a channel. */
  isPushChannelMuted: (channelId: number) => boolean;

  // DM actions
  selectDmUser: (session: number) => Promise<void>;
  sendDm: (targetSession: number, body: string) => Promise<void>;
  refreshDmMessages: (session: number) => Promise<void>;

  // Persistent chat actions
  fetchHistory: (channelId: number, beforeId?: string) => Promise<void>;
  getPersistenceMode: (channelId: number) => PersistenceMode;
  verifyKeyFingerprint: (channelId: number) => Promise<void>;
  acceptCustodianChanges: (channelId: number) => Promise<void>;
  confirmCustodians: (channelId: number) => Promise<void>;
  resolveKeyDispute: (channelId: number, trustedSenderHash: string) => Promise<void>;
  updateChannelPersistenceConfig: (channelId: number, config: ChannelPersistConfig) => void;
  approveKeyShare: (channelId: number, peerCertHash: string) => Promise<void>;
  dismissKeyShare: (channelId: number, peerCertHash: string) => Promise<void>;
  queryKeyHolders: (channelId: number) => Promise<void>;

  // Message deletion
  deletePchatMessages: (channelId: number, opts: {
    messageIds?: string[];
    timeFrom?: number;
    timeTo?: number;
    senderHash?: string;
  }) => Promise<void>;
}

const INITIAL: Pick<
  AppState,
  | "status"
  | "channels"
  | "users"
  | "selectedChannel"
  | "currentChannel"
  | "selectedUser"
  | "ownSession"
  | "messages"
  | "error"
  | "listenedChannels"
  | "unreadCounts"
  | "serverConfig"
  | "fileServerConfig"
  | "fileServerCapabilities"
  | "customServerEmotes"
  | "downloads"
  | "unseenDownloadCount"
  | "serverFancyVersion"
  | "voiceState"
  | "udpActive"
  | "inCall"
  | "talkingSessions"
  | "selectedDmUser"
  | "dmMessages"
  | "dmUnreadCounts"
  | "polls"
  | "pollMessages"
  | "pendingMessages"
  | "linkEmbeds"
  | "reactionVersion"
  | "unseenPinIds"
  | "readReceiptVersion"
  | "typingUsers"
  | "watchSessions"
  | "watchSessionsVersion"
  | "isSharingOwn"
  | "broadcastingOwnSession"
  | "webrtcConnecting"
  | "webrtcError"
  | "desktopDrawingOverlayOpen"
  | "drawingActiveChannels"
  | "broadcastingSessions"
  | "watchingSession"
  | "watchingOwnSession"
  | "channelPersistence"
  | "keyTrust"
  | "custodianPins"
  | "pendingDisputes"
  | "pchatHistoryLoading"
  | "pendingKeyShares"
  | "keyHolders"
  | "pchatKeyRevoked"
  | "signalBridgeError"
  | "silencedChannels"
  | "mutedPushChannels"
  | "pushSubscribedChannels"
  | "userVolumes"
  | "serverLog"
  | "passwordRequired"
  | "passwordAttempted"
  | "pendingConnect"
  | "connectedCertLabel"
  | "bootstrapStage"
> = {
  status: "disconnected",
  channels: [],
  users: [],
  selectedChannel: null,
  currentChannel: null,
  selectedUser: null,
  ownSession: null,
  messages: [],
  error: null,
  listenedChannels: new Set(),
  unreadCounts: {},
  serverConfig: {
    max_message_length: 5000,
    max_image_message_length: 131072,
    allow_html: true,
    webrtc_sfu_available: false,
    fancy_rest_api_url: null,
  },
  fileServerConfig: null,
  fileServerCapabilities: null,
  customServerEmotes: [],
  downloads: [],
  unseenDownloadCount: 0,
  serverFancyVersion: null,
  voiceState: "inactive" as VoiceState,
  udpActive: false,
  inCall: false,
  talkingSessions: new Set<number>(),
  selectedDmUser: null,
  dmMessages: [],
  dmUnreadCounts: {},
  polls: new Map(),
  pollMessages: [],
  pendingMessages: [],
  linkEmbeds: new Map(),
  reactionVersion: 0,
  unseenPinIds: new Map(),
  readReceiptVersion: 0,
  typingUsers: new Map(),
  watchSessions: new Map(),
  watchSessionsVersion: 0,
  isSharingOwn: false,
  broadcastingOwnSession: null,
  webrtcConnecting: false,
  webrtcError: null,
  desktopDrawingOverlayOpen: false,
  drawingActiveChannels: new Set(),
  broadcastingSessions: new Set(),
  watchingSession: null,
  watchingOwnSession: null,
  channelPersistence: {},
  keyTrust: {},
  custodianPins: {},
  pendingDisputes: {},
  pchatHistoryLoading: new Set(),
  pendingKeyShares: {},
  keyHolders: {},
  pchatKeyRevoked: new Set(),
  signalBridgeError: null,
  silencedChannels: new Set(),
  mutedPushChannels: new Set(),
  pushSubscribedChannels: new Set(),
  userVolumes: {},
  serverLog: [],
  passwordRequired: false,
  passwordAttempted: false,
  pendingConnect: null,
  connectedCertLabel: null,
  bootstrapStage: null,
};

// --- Store --------------------------------------------------------

/**
 * Monotonically increasing sequence number for channel-message writes.
 * Every async operation that sets `messages` bumps this counter before
 * starting the IPC round-trip and only applies the result when the
 * counter hasn't been bumped again in the meantime.  This prevents
 * stale `get_messages` responses from overwriting fresher data.
 */
let messageWriteSeq = 0;

/**
 * Threshold (in HTML body length) above which a sent message gets an
 * optimistic placeholder + progress UI even when it doesn't contain an
 * image.  Picked so plain chat messages never trigger the indicator
 * but anything that is likely to take noticeable time on a slow link
 * does.
 */
const LARGE_MESSAGE_THRESHOLD = 4096;

/** Whether a message body should show an optimistic upload-progress UI. */
function bodyNeedsProgressUI(body: string): boolean {
  if (body.includes("<img")) return true;
  if (body.includes("<video")) return true;
  return body.length > LARGE_MESSAGE_THRESHOLD;
}

function newPendingId(): string {
  return globalThis.crypto?.randomUUID?.()
    ?? `pending-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

/** Update the taskbar badge with the total unread count (channels + DMs). */
function updateBadgeCount(): void {
  const { unreadCounts, dmUnreadCounts, silencedChannels } = useAppStore.getState();
  const channelSum = Object.entries(unreadCounts)
    .filter(([id]) => !silencedChannels.has(Number(id)))
    .reduce((a, [, b]) => a + b, 0);
  const dmSum = Object.values(dmUnreadCounts).reduce((a, b) => a + b, 0);
  const total = channelSum + dmSum;
  invoke("update_badge_count", { count: total > 0 ? total : null }).catch(() => {
    // Badge API may not be available on all platforms.
  });
}

export const useAppStore = create<AppState>((set, get) => ({
  ...INITIAL,
  disableLinkPreviews: false,
  enableExternalEmbeds: false,
  streamerMode: false,

  // Multi-server (Phase C): outside INITIAL so it survives single-session disconnects.
  sessions: [],
  activeServerId: null,
  sessionUnreadTotals: {},
  sessionErrors: {},

  refreshSessions: async () => {
    try {
      const [sessions, activeServerId] = await Promise.all([
        invoke<import("./types").SessionMeta[]>("list_servers"),
        invoke<import("./types").ServerId | null>("get_active_server"),
      ]);
      set((prev) => {
        // Drop per-tab badge entries for sessions that no longer exist.
        const ids = new Set(sessions.map((s) => s.id));
        const next: Record<string, number> = {};
        for (const [k, v] of Object.entries(prev.sessionUnreadTotals)) {
          const baseId = k.split(":")[0];
          if (ids.has(baseId)) next[k] = v;
        }
        // Drop stored errors for sessions that no longer exist.
        const nextErrors: Record<string, string | null> = {};
        for (const [k, v] of Object.entries(prev.sessionErrors)) {
          if (ids.has(k)) nextErrors[k] = v;
        }
        return { sessions, activeServerId, sessionUnreadTotals: next, sessionErrors: nextErrors };
      });
    } catch (e) {
      console.error("refreshSessions error:", e);
    }
  },

  switchServer: async (id) => {
    try {
      await invoke("set_active_server", { serverId: id });
      // Clear our per-tab badge cache for the newly-active session;
      // its unreads now live in `unreadCounts` / `dmUnreadCounts`.
      set((prev) => {
        const next = { ...prev.sessionUnreadTotals };
        delete next[id];
        delete next[`${id}:ch`];
        delete next[`${id}:dm`];
        return { activeServerId: id, sessionUnreadTotals: next };
      });
      // Sync global status/error from this session's own metadata so the
      // ChatPage overlay reflects the tab the user just switched to,
      // not whatever the previously-active tab's status was.
      await get().refreshSessions().catch(() => {});
      const { sessions, sessionErrors } = get();
      const meta = sessions.find((s) => s.id === id);
      const sessionStatus = meta?.status ?? "disconnected";
      set({
        status: sessionStatus,
        error: sessionStatus === "connected" ? null : (sessionErrors[id] ?? null),
      });
      // Repopulate per-session data for the newly-active session.
      await get().refreshState();
      try {
        const currentCh = await invoke<number | null>("get_current_channel");
        set({ currentChannel: currentCh, selectedChannel: currentCh });
        if (currentCh !== null) {
          const messages = await invoke<ChatMessage[]>("get_messages", { channelId: currentCh });
          set({ messages });
        } else {
          set({ messages: [] });
        }
      } catch (e) {
        console.error("switchServer post-switch refresh error:", e);
      }
      try {
        const ownSession = await invoke<number | null>("get_own_session");
        set({ ownSession });
      } catch {
        // not connected; leave as-is.
      }
    } catch (e) {
      console.error("switchServer error:", e);
      throw e;
    }
  },

  disconnectSession: async (id) => {
    intentionallyClosingSessions.add(id);
    const wasActive = get().activeServerId === id;
    try {
      await invoke("disconnect_server", { serverId: id });
    } catch (e) {
      console.error("disconnectSession error:", e);
      intentionallyClosingSessions.delete(id);
      throw e;
    }
    // Drop the cached error for the closed session.
    set((prev) => {
      if (prev.sessionErrors[id] == null) return prev;
      const next = { ...prev.sessionErrors };
      delete next[id];
      return { sessionErrors: next };
    });
    // Refresh the sessions list and learn which session (if any) the
    // backend made active in place of the one we just closed.
    await get().refreshSessions().catch(() => {});
    const { sessions: nextSessions, activeServerId: nextActive } = get();
    if (wasActive) {
      if (nextActive && nextSessions.some((s) => s.id === nextActive)) {
        // The backend rebound the active session to a remaining one.
        // Reflect its status / error / data in the global store.
        await get().switchServer(nextActive).catch(() => {});
      } else {
        // No sessions left - reset to the empty connect-page state.
        manualDisconnectRequested = true;
        offloadManager.dispose().catch(() => {});
        volumeAppliedSessions.clear();
        clearReadReceipts();
        set({ ...INITIAL });
        invoke("update_badge_count", { count: null }).catch(() => {});
        navigateRef?.("/");
      }
    }
    intentionallyClosingSessions.delete(id);
  },

  connect: async (host, port, username, certLabel, password) => {
    manualDisconnectRequested = false;
    clearAutoReconnectTimer();
    set({
      status: "connecting",
      error: null,
      passwordRequired: false,
      pendingConnect: { host, port, username, certLabel: certLabel ?? null },
      connectedCertLabel: certLabel ?? null,
      bootstrapStage: "Negotiating with server...",
    });
    try {
      await invoke("connect", {
        host,
        port,
        username,
        certLabel: certLabel ?? null,
        password: password ?? null,
      });
      // Sync activeServerId before rejection events arrive, so listener
      // routing works even if the new session id isn't known yet.
      await get().refreshSessions().catch(() => {});
    } catch (e) {
      set({
        status: "disconnected",
        error: String(e),
        pendingConnect: null,
        connectedCertLabel: null,
        bootstrapStage: null,
      });
    }
  },

  disconnect: async () => {
    clearAutoReconnectTimer();
    const activeId = get().activeServerId;
    if (activeId) {
      // Delegate to the multi-session-aware path so closing the active
      // session via the sidebar button behaves identically to closing
      // it via the tab close button: the backend rebinds `inner` to
      // the next session and the UI follows along instead of flashing
      // a misleading "Connection lost" overlay on the next tab.
      try {
        await get().disconnectSession(activeId);
      } catch (e) {
        console.error("disconnect error:", e);
      }
      return;
    }
    // No active session - fall back to a full local reset.
    manualDisconnectRequested = true;
    try {
      await offloadManager.dispose();
      await invoke("disconnect");
    } catch (e) {
      console.error("disconnect error:", e);
    }
    resetReactions();
    clearReadReceipts();
    set({ ...INITIAL });
    invoke("update_badge_count", { count: null }).catch(() => {});
    useAppStore.getState().refreshSessions().catch(() => {});
  },

  selectChannel: async (id) => {
    set({ selectedChannel: id, selectedDmUser: null, dmMessages: [] });
    const seq = ++messageWriteSeq;
    try {
      // Notify backend - marks channel as read and clears DM selection.
      await invoke("select_channel", { channelId: id });
      const messages = await invoke<ChatMessage[]>("get_messages", {
        channelId: id,
      });
      // Only apply if no newer write has started (avoids overwriting
      // fresher data from a concurrent refreshMessages / new-message).
      if (messageWriteSeq === seq) {
        set({ messages });
      }
    } catch (e) {
      console.error("select_channel error:", e);
    }
  },

  joinChannel: async (id) => {
    try {
      await invoke("join_channel", { channelId: id });
      set({ currentChannel: id });
    } catch (e) {
      console.error("join_channel error:", e);
    }
  },

  createChannel: async (parentId, name, opts = {}) => {
    try {
      await invoke("create_channel", {
        parentId,
        name,
        description: opts.description ?? null,
        position: opts.position ?? null,
        temporary: opts.temporary ?? null,
        maxUsers: opts.maxUsers ?? null,
        pchatProtocol: opts.pchatProtocol ?? null,
        pchatMaxHistory: opts.pchatMaxHistory ?? null,
        pchatRetentionDays: opts.pchatRetentionDays ?? null,
      });
    } catch (e) {
      console.error("create_channel error:", e);
      throw e;
    }
  },

  updateChannel: async (channelId, opts) => {
    try {
      await invoke("update_channel", {
        channelId,
        name: opts.name ?? null,
        description: opts.description ?? null,
        position: opts.position ?? null,
        temporary: opts.temporary ?? null,
        maxUsers: opts.maxUsers ?? null,
        pchatProtocol: opts.pchatProtocol ?? null,
        pchatMaxHistory: opts.pchatMaxHistory ?? null,
        pchatRetentionDays: opts.pchatRetentionDays ?? null,
      });
    } catch (e) {
      console.error("update_channel error:", e);
      throw e;
    }
  },

  deleteChannel: async (channelId) => {
    try {
      await invoke("delete_channel", { channelId });
    } catch (e) {
      console.error("delete_channel error:", e);
      throw e;
    }
  },

  sendMessage: async (channelId, body) => {
    const pendingId = newPendingId();
    const showPlaceholder = bodyNeedsProgressUI(body);
    if (showPlaceholder) {
      set((s) => ({
        pendingMessages: [
          ...s.pendingMessages,
          {
            pendingId,
            channelId,
            dmSession: null,
            body,
            createdAt: Date.now(),
            state: "sending",
          },
        ],
      }));
    }
    try {
      await invoke("send_message", { channelId, body });
      const seq = ++messageWriteSeq;
      const messages = await invoke<ChatMessage[]>("get_messages", {
        channelId,
      });
      const updates: Partial<AppState> = {};
      if (messageWriteSeq === seq) {
        updates.messages = messages;
      }
      if (showPlaceholder) {
        set((s) => ({
          ...updates,
          pendingMessages: s.pendingMessages.filter((p) => p.pendingId !== pendingId),
        }));
      } else if (Object.keys(updates).length > 0) {
        set(updates);
      }
    } catch (e) {
      console.error("send_message error:", e);
      if (showPlaceholder) {
        const detail = e instanceof Error ? e.message : String(e);
        set((s) => ({
          pendingMessages: s.pendingMessages.map((p) =>
            p.pendingId === pendingId
              ? { ...p, state: "failed" as const, errorMessage: detail }
              : p,
          ),
        }));
      }
    }
  },

  dismissPendingMessage: (pendingId) => {
    set((s) => ({
      pendingMessages: s.pendingMessages.filter((p) => p.pendingId !== pendingId),
    }));
  },

  addPendingPlaceholder: (channelId, dmSession, body) => {
    const pendingId = newPendingId();
    set((s) => ({
      pendingMessages: [
        ...s.pendingMessages,
        {
          pendingId,
          channelId,
          dmSession,
          body,
          createdAt: Date.now(),
          state: "sending",
        },
      ],
    }));
    return pendingId;
  },

  markPendingFailed: (pendingId, errorMessage) => {
    set((s) => ({
      pendingMessages: s.pendingMessages.map((p) =>
        p.pendingId === pendingId
          ? { ...p, state: "failed" as const, errorMessage }
          : p,
      ),
    }));
  },

  retryPendingMessage: async (pendingId) => {
    const target = get().pendingMessages.find((p) => p.pendingId === pendingId);
    if (!target) return;
    set((s) => ({
      pendingMessages: s.pendingMessages.filter((p) => p.pendingId !== pendingId),
    }));
    if (target.dmSession !== null) {
      await get().sendDm(target.dmSession, target.body);
    } else if (target.channelId !== null) {
      await get().sendMessage(target.channelId, target.body);
    }
  },

  editMessage: async (channelId, messageId, newBody) => {
    try {
      await invoke("edit_message", { channelId, messageId, newBody });
      const seq = ++messageWriteSeq;
      const messages = await invoke<ChatMessage[]>("get_messages", {
        channelId,
      });
      if (messageWriteSeq === seq) {
        set({ messages });
      }
    } catch (e) {
      console.error("edit_message error:", e);
    }
  },

  refreshState: async () => {
    try {
      const [channels, users, pushSubscribed] = await Promise.all([
        invoke<ChannelEntry[]>("get_channels"),
        invoke<UserEntry[]>("get_users"),
        invoke<number[]>("get_push_subscribed_channels"),
      ]);

      // Derive channelPersistence from channel pchat_protocol so the
      // PersistenceBanner (and its loading indicator) can render.
      const prev = get().channelPersistence;
      const nextPersistence: Record<number, ChannelPersistenceState> = { ...prev };
      for (const ch of channels) {
        if (ch.pchat_protocol && ch.pchat_protocol !== "none") {
          const mode = ch.pchat_protocol.toUpperCase() as PersistenceMode;
          nextPersistence[ch.id] = {
            mode,
            maxHistory: ch.pchat_max_history ?? prev[ch.id]?.maxHistory ?? 0,
            retentionDays: ch.pchat_retention_days ?? prev[ch.id]?.retentionDays ?? 0,
            hasMore: prev[ch.id]?.hasMore ?? false,
            isFetching: prev[ch.id]?.isFetching ?? false,
            totalStored: prev[ch.id]?.totalStored ?? 0,
          };
        }
      }
      set({ channels, users, channelPersistence: nextPersistence, pushSubscribedChannels: new Set(pushSubscribed) });

      // Clean up broadcastingSessions for users that are no longer connected.
      const currentSessions = new Set(users.map((u) => u.session));
      const { broadcastingSessions } = get();
      if (broadcastingSessions.size > 0) {
        const pruned = new Set([...broadcastingSessions].filter((s) => currentSessions.has(s)));
        if (pruned.size !== broadcastingSessions.size) {
          // Wipe drawings authored by anyone who disconnected mid-stream
          // so their annotations vanish for every viewer.  Imported
          // lazily to avoid a circular module dependency between the
          // store and the chat-only DrawingOverlay module.
          const dropped = [...broadcastingSessions].filter((s) => !currentSessions.has(s));
          if (dropped.length > 0) {
            void import("./components/chat/DrawingOverlay").then((m) => {
              for (const s of dropped) m.clearStrokesFromSender(s);
            }).catch(() => {});
          }
          set({ broadcastingSessions: pruned });
        }
      }
    } catch (e) {
      console.error("refresh error:", e);
    }
  },

  refreshMessages: async (channelId) => {
    const seq = ++messageWriteSeq;
    try {
      const messages = await invoke<ChatMessage[]>("get_messages", {
        channelId,
      });
      if (messageWriteSeq === seq) {
        set({ messages });
      }
    } catch (e) {
      console.error("refresh messages error:", e);
    }
  },

  toggleListen: async (channelId) => {
    try {
      const isNowListened = await invoke<boolean>("toggle_listen", {
        channelId,
      });
      set((prev) => {
        const next = new Set(prev.listenedChannels);
        if (isNowListened) next.add(channelId);
        else next.delete(channelId);
        return { listenedChannels: next };
      });
    } catch (e) {
      console.error("toggle_listen error:", e);
    }
  },

  enableVoice: async () => {
    try {
      await invoke("enable_voice");
      set({ voiceState: "active", inCall: true });
      updatePreferences({ voiceOnReconnect: true }).catch(() => {});
    } catch (e) {
      console.error("enable_voice error:", e);
    }
  },

  disableVoice: async () => {
    try {
      await invoke("disable_voice");
      set({ voiceState: "inactive", inCall: false, talkingSessions: new Set() });
      updatePreferences({ voiceOnReconnect: false, voiceMutedOnReconnect: false }).catch(() => {});
    } catch (e) {
      console.error("disable_voice error:", e);
    }
  },

  toggleMute: async () => {
    // Capture state BEFORE the await so pref write is deterministic and
    // ordered relative to the user action, not the async Rust IPC delivery.
    // "active" → will be muted; "muted" or "inactive" → will be active.
    const willBeMuted = useAppStore.getState().voiceState === "active";
    try {
      await invoke("toggle_mute");
      if (!isRestoringVoice) {
        updatePreferences({ voiceOnReconnect: true, voiceMutedOnReconnect: willBeMuted }).catch(() => {});
      }
    } catch (e) {
      console.error("toggle_mute error:", e);
    }
  },

  toggleDeafen: async () => {
    try {
      await invoke("toggle_deafen");
    } catch (e) {
      console.error("toggle_deafen error:", e);
    }
  },

  selectUser: (session) => set({ selectedUser: session }),

  selectDmUser: async (session) => {
    // Toggle: clicking the currently-selected DM user a second time
    // switches back to the channel the local user is currently in.
    const { selectedDmUser, currentChannel, selectChannel } = get();
    if (selectedDmUser === session) {
      if (currentChannel == null) {
        set({ selectedDmUser: null, dmMessages: [], selectedUser: null });
      } else {
        await selectChannel(currentChannel);
        set({ selectedUser: null });
      }
      return;
    }
    set({ selectedDmUser: session, selectedChannel: null, messages: [], selectedUser: session });
    try {
      await invoke("select_dm_user", { session });
      const dmMessages = await invoke<ChatMessage[]>("get_dm_messages", { session });
      set({ dmMessages });
    } catch (e) {
      console.error("select_dm_user error:", e);
    }
  },

  sendDm: async (targetSession, body) => {
    const pendingId = newPendingId();
    const showPlaceholder = bodyNeedsProgressUI(body);
    if (showPlaceholder) {
      set((s) => ({
        pendingMessages: [
          ...s.pendingMessages,
          {
            pendingId,
            channelId: null,
            dmSession: targetSession,
            body,
            createdAt: Date.now(),
            state: "sending",
          },
        ],
      }));
    }
    try {
      await invoke("send_dm", { targetSession, body });
      const dmMessages = await invoke<ChatMessage[]>("get_dm_messages", { session: targetSession });
      if (showPlaceholder) {
        set((s) => ({
          dmMessages,
          pendingMessages: s.pendingMessages.filter((p) => p.pendingId !== pendingId),
        }));
      } else {
        set({ dmMessages });
      }
    } catch (e) {
      console.error("send_dm error:", e);
      if (showPlaceholder) {
        const detail = e instanceof Error ? e.message : String(e);
        set((s) => ({
          pendingMessages: s.pendingMessages.map((p) =>
            p.pendingId === pendingId
              ? { ...p, state: "failed" as const, errorMessage: detail }
              : p,
          ),
        }));
      }
    }
  },

  refreshDmMessages: async (session) => {
    try {
      const dmMessages = await invoke<ChatMessage[]>("get_dm_messages", { session });
      set({ dmMessages });
    } catch (e) {
      console.error("refresh dm messages error:", e);
    }
  },

  sendPluginData: async (receiverSessions, data, dataId) => {
    try {
      await invoke("send_plugin_data", {
        receiverSessions,
        data: Array.from(data),
        dataId,
      });
    } catch (e) {
      console.error("send_plugin_data error:", e);
    }
  },

  uploadFile: async ({ filePath, channelId, mode, password, filename, mimeType, uploadId }) => {
    const cfg = get().fileServerConfig;
    if (!cfg) {
      throw new Error("file-server is not configured for this server");
    }
    if (mode === "password" && !password) {
      throw new Error("mode=password requires a password");
    }
    const resp = await invoke<UploadResponse>("upload_file", {
      request: {
        baseUrl: cfg.baseUrl,
        session: cfg.sessionId,
        uploadToken: cfg.uploadToken,
        channelId,
        filePath,
        filename,
        mimeType,
        mode,
        password,
        uploadId: uploadId ?? "",
      },
    });
    return { ...resp, download_url: rebaseFileServerUrl(resp.download_url) };
  },

  downloadFile: async ({ url, destPath, password }) => {
    const cfg = get().fileServerConfig;
    let credential: { kind: "password" | "session"; value: string } | undefined;
    if (password !== undefined) {
      credential = { kind: "password", value: password };
    } else if (cfg?.sessionJwt) {
      credential = { kind: "session", value: cfg.sessionJwt };
    }
    return await invoke<number>("download_file", {
      request: {
        url,
        destPath,
        credential,
      },
    });
  },

  sendWebRtcSignal: async (targetSession, signalType, payload, serverId) => {
    try {
      await invoke("send_webrtc_signal", {
        targetSession,
        signalType,
        payload,
        serverId: serverId ?? null,
      });
    } catch (e) {
      console.error("send_webrtc_signal error:", e);
    }
  },

  sendReaction: async (channelId, messageId, emoji, action) => {
    try {
      await invoke("send_reaction", { channelId, messageId, emoji, action });
    } catch (e) {
      console.error("send_reaction error:", e);
    }
  },

  pinMessage: async (channelId, messageId, unpin) => {
    try {
      await invoke("pin_message", { channelId, messageId, unpin });
    } catch (e) {
      console.error("pin_message error:", e);
    }
  },

  clearUnseenPins: (channelId) => {
    set((s) => {
      const next = new Map(s.unseenPinIds);
      next.delete(channelId);
      return { unseenPinIds: next };
    });
  },

  addDownload: (entry) => {
    const id = (globalThis.crypto?.randomUUID?.() ?? `dl-${Date.now()}-${Math.random().toString(36).slice(2)}`);
    const full: DownloadEntry = { ...entry, id, downloadedAt: Date.now() };
    set((s) => ({
      downloads: [full, ...s.downloads].slice(0, 200),
      unseenDownloadCount: s.unseenDownloadCount + 1,
    }));
  },

  markDownloadsSeen: () => {
    set({ unseenDownloadCount: 0 });
  },

  removeDownload: (id) => {
    set((s) => ({ downloads: s.downloads.filter((d) => d.id !== id) }));
  },

  clearDownloads: () => {
    set({ downloads: [], unseenDownloadCount: 0 });
  },

  addCustomEmote: async ({ shortcode, aliasEmoji, description, filePath, mimeType }) => {
    const cfg = get().fileServerConfig;
    if (!cfg) throw new Error("file-server is not configured for this server");
    if (!cfg.canManageEmotes) throw new Error("you are not allowed to manage emotes");
    await invoke("add_custom_emote", {
      request: {
        baseUrl: cfg.baseUrl,
        sessionJwt: cfg.sessionJwt,
        shortcode,
        aliasEmoji,
        description,
        filePath,
        mimeType,
      },
    });
  },

  removeCustomEmote: async (shortcode) => {
    const cfg = get().fileServerConfig;
    if (!cfg) throw new Error("file-server is not configured for this server");
    if (!cfg.canManageEmotes) throw new Error("you are not allowed to manage emotes");
    await invoke("remove_custom_emote", {
      request: {
        baseUrl: cfg.baseUrl,
        sessionJwt: cfg.sessionJwt,
        shortcode,
      },
    });
  },

  addPoll: (poll, isOwn) => {
    registerPoll(poll);
    set((prev) => {
      const newPolls = new Map(prev.polls).set(poll.id, poll);
      // Avoid duplicate synthetic messages.
      if (prev.pollMessages.some((m) => m.body.includes(poll.id))) {
        return { polls: newPolls };
      }
      return {
        polls: newPolls,
        pollMessages: [
          ...prev.pollMessages,
          {
            sender_session: poll.creator,
            sender_name: poll.creatorName || "Unknown",
            body: `<!-- FANCY_POLL:${poll.id} -->`,
            channel_id: poll.channelId ?? 0,
            is_own: isOwn,
          },
        ],
      };
    });
  },
  setError: (error) => set({ error }),
  reset: () => set({ ...INITIAL }),

  retryWithPassword: async (password) => {
    const pending = get().pendingConnect;
    if (!pending) return;
    set({ passwordRequired: false, passwordAttempted: true, pendingConnect: null });
    await get().connect(pending.host, pending.port, pending.username, pending.certLabel, password);
  },

  dismissPasswordPrompt: () => {
    set({ passwordRequired: false, passwordAttempted: false, pendingConnect: null, connectedCertLabel: null });
  },

  // -- Silenced channels ------------------------------------------

  toggleSilenceChannel: async (channelId) => {
    const { silencedChannels, pendingConnect } = get();
    if (!pendingConnect) return false;
    const serverKey = `${pendingConnect.host}:${pendingConnect.port}`;
    const isSilenced = silencedChannels.has(channelId);
    const updated = await setSilencedChannel(serverKey, channelId, !isSilenced);
    set({ silencedChannels: new Set(updated) });
    updateBadgeCount();
    return !isSilenced;
  },

  isChannelSilenced: (channelId) => {
    return get().silencedChannels.has(channelId);
  },

  // -- Push notification muting -----------------------------------

  toggleMutePushChannel: async (channelId) => {
    const { mutedPushChannels, pendingConnect } = get();
    if (!pendingConnect) return false;
    const serverKey = `${pendingConnect.host}:${pendingConnect.port}`;
    const isMuted = mutedPushChannels.has(channelId);
    const updated = await setMutedPushChannel(serverKey, channelId, !isMuted);
    set({ mutedPushChannels: new Set(updated) });

    // Sync the muted list to the server via native proto message.
    try {
      await invoke("send_push_update", { mutedChannels: updated });
    } catch (e) {
      console.error("Failed to sync push mute to server:", e);
    }

    return !isMuted;
  },

  isPushChannelMuted: (channelId) => {
    return get().mutedPushChannels.has(channelId);
  },

  // -- Per-user volume overrides ----------------------------------

  setUserVolume: (hash, volume) => {
    const next = { ...get().userVolumes };
    if (volume === 100) {
      delete next[hash];
    } else {
      next[hash] = volume;
    }
    set({ userVolumes: next });
    saveUserVolume(hash, volume).catch((err) =>
      console.error("saveUserVolume failed:", err),
    );
  },

  // -- Persistent chat actions ------------------------------------

  fetchHistory: async (channelId, beforeId) => {
    set((prev) => ({
      channelPersistence: {
        ...prev.channelPersistence,
        [channelId]: {
          ...prev.channelPersistence[channelId],
          isFetching: true,
        },
      },
    }));
    try {
      // Fire-and-forget: the response arrives asynchronously via
      // "pchat-fetch-complete" and "new-message" events.
      await invoke<void>("fetch_older_messages", {
        channelId,
        beforeId: beforeId ?? null,
        limit: 50,
      });
    } catch (e) {
      console.error("fetch_older_messages error:", e);
      set((prev) => ({
        channelPersistence: {
          ...prev.channelPersistence,
          [channelId]: {
            ...prev.channelPersistence[channelId],
            isFetching: false,
          },
        },
      }));
    }
  },

  getPersistenceMode: (channelId) => {
    return get().channelPersistence[channelId]?.mode ?? "NONE";
  },

  verifyKeyFingerprint: async (channelId) => {
    try {
      await invoke("verify_channel_key_manual", { channelId });
      set((prev) => ({
        keyTrust: {
          ...prev.keyTrust,
          [channelId]: {
            ...prev.keyTrust[channelId],
            trustLevel: "ManuallyVerified",
          },
        },
      }));
    } catch (e) {
      console.error("verify_channel_key_manual error:", e);
    }
  },

  acceptCustodianChanges: async (channelId) => {
    try {
      await invoke("accept_custodian_changes", { channelId });
      set((prev) => {
        const pin = prev.custodianPins[channelId];
        if (!pin?.pendingUpdate) return {};
        return {
          custodianPins: {
            ...prev.custodianPins,
            [channelId]: {
              pinned: pin.pendingUpdate,
              confirmed: true,
              pendingUpdate: null,
            },
          },
        };
      });
    } catch (e) {
      console.error("accept_custodian_changes error:", e);
    }
  },

  confirmCustodians: async (channelId) => {
    try {
      const { custodianPins } = get();
      const pin = custodianPins[channelId];
      if (!pin) return;
      await invoke("confirm_custodians", {
        channelId,
        custodianHashes: pin.pinned,
      });
      set((prev) => ({
        custodianPins: {
          ...prev.custodianPins,
          [channelId]: { ...prev.custodianPins[channelId], confirmed: true },
        },
      }));
    } catch (e) {
      console.error("confirm_custodians error:", e);
    }
  },

  resolveKeyDispute: async (channelId, trustedSenderHash) => {
    try {
      await invoke("resolve_key_dispute", { channelId, trustedSenderHash });
      set((prev) => {
        const { [channelId]: _removed, ...rest } = prev.pendingDisputes;
        return {
          pendingDisputes: rest,
          keyTrust: {
            ...prev.keyTrust,
            [channelId]: {
              ...prev.keyTrust[channelId],
              trustLevel: "ManuallyVerified",
            },
          },
        };
      });
    } catch (e) {
      console.error("resolve_key_dispute error:", e);
    }
  },

  updateChannelPersistenceConfig: (channelId, config) => {
    set((prev) => ({
      channelPersistence: {
        ...prev.channelPersistence,
        [channelId]: {
          mode: config.mode,
          maxHistory: config.maxHistory,
          retentionDays: config.retentionDays,
          hasMore: false,
          isFetching: false,
          totalStored: prev.channelPersistence[channelId]?.totalStored ?? 0,
        },
      },
    }));
  },

  approveKeyShare: async (channelId, peerCertHash) => {
    try {
      await invoke("approve_key_share", { channelId, peerCertHash });
    } catch (e) {
      console.error("approve_key_share error:", e);
    }
  },

  dismissKeyShare: async (channelId, peerCertHash) => {
    try {
      await invoke("dismiss_key_share", { channelId, peerCertHash });
    } catch (e) {
      console.error("dismiss_key_share error:", e);
    }
  },

  queryKeyHolders: async (channelId) => {
    try {
      await invoke("query_key_holders", { channelId });
    } catch (e) {
      console.error("query_key_holders error:", e);
    }
  },

  deletePchatMessages: async (channelId, opts) => {
    try {
      await invoke("delete_pchat_messages", {
        channelId,
        messageIds: opts.messageIds ?? [],
        timeFrom: opts.timeFrom ?? null,
        timeTo: opts.timeTo ?? null,
        senderHash: opts.senderHash ?? null,
      });

      // The invoke resolves only after the server's PchatAck confirms
      // success, so it is safe to remove the messages locally now.
      if (opts.messageIds && opts.messageIds.length > 0) {
        const removed = new Set(opts.messageIds);
        set((prev) => ({
          messages: prev.messages.filter(
            (m) => !m.message_id || !removed.has(m.message_id),
          ),
        }));
      } else {
        // For time-range or sender-hash deletions we cannot determine
        // which messages were affected locally, so re-fetch from the
        // backend.
        await get().refreshMessages(channelId);
      }
    } catch (e) {
      console.error("delete_pchat_messages error:", e);
      throw e;
    }
  },
}));

// --- Tauri event bridge -------------------------------------------

// --- Plugin data handler registry ---------------------------------

type PluginDataHandler = (dataId: string, data: Uint8Array, senderSession: number | null) => void;
const pluginDataHandlers: PluginDataHandler[] = [];

/** Register a handler for incoming plugin data transmissions. */
export function onPluginData(handler: PluginDataHandler): () => void {
  pluginDataHandlers.push(handler);
  return () => {
    const idx = pluginDataHandlers.indexOf(handler);
    if (idx >= 0) pluginDataHandlers.splice(idx, 1);
  };
}

// --- WebRTC signal handler registry ---

type WebRtcSignalHandler = (senderSession: number | null, targetSession: number | null, signalType: number, payload: string, serverId: string | null) => void;
const webRtcSignalHandlers: WebRtcSignalHandler[] = [];

/** Register a handler for incoming WebRTC screen-sharing signals. */
export function onWebRtcSignal(handler: WebRtcSignalHandler): () => void {
  webRtcSignalHandlers.push(handler);
  return () => {
    const idx = webRtcSignalHandlers.indexOf(handler);
    if (idx >= 0) webRtcSignalHandlers.splice(idx, 1);
  };
}

/** Set of request_ids already sent to avoid duplicate requests. */
const pendingPreviewRequests = new Set<string>();

/** Request link previews from the server for the given URLs. */
export async function requestLinkPreview(urls: string[], requestId: string): Promise<void> {
  if (pendingPreviewRequests.has(requestId)) return;
  pendingPreviewRequests.add(requestId);
  try {
    await invoke("request_link_preview", { urls, requestId });
  } catch (e) {
    console.error("request_link_preview failed:", e);
    pendingPreviewRequests.delete(requestId);
  }
}

/**
 * Subscribe to backend events and translate them into store updates.
 * Call once from the root `<App>` component; returns cleanup functions.
 */
export async function initEventListeners(
  navigate: (path: string) => void,
): Promise<UnlistenFn[]> {
  navigateRef = navigate;
  const unlisteners: UnlistenFn[] = [];

  // Bootstrap the multi-server session list once at startup so the
  // sessions slice reflects whatever the backend already has.
  useAppStore.getState().refreshSessions().catch(() => {});

  // Ensure notification permissions and channel are set up (Android 8+ / 13+).
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      const result = await requestPermission();
      granted = result === "granted";
    }
    if (granted) {
      await createChannel({
        id: "messages",
        name: "Messages",
        description: "Chat message notifications",
        importance: Importance.High,
        visibility: Visibility.Public,
      });
    }
  } catch {
    // Notification API may not be available on all platforms.
  }

  // Sync the notification preference to the Rust backend.
  try {
    const { getPreferences } = await import("./preferencesStorage");
    const prefs = await getPreferences();
    await invoke("set_notifications_enabled", {
      enabled: prefs.enableNotifications ?? true,
    });
    await invoke("set_disable_dual_path", {
      disabled: !(prefs.enableDualPath ?? false),
    });
    const logLevel = prefs.logLevel ?? (prefs.debugLogging ? "debug" : "info");
    if (logLevel !== "info") {
      await invoke("set_log_level", { filter: logLevel });
    }
  } catch {
    // Preference store may not be ready yet - backend defaults to enabled.
  }

  // Server fully connected (ServerSync received).
  unlisteners.push(
    await listen("server-connected", async () => {
      manualDisconnectRequested = false;
      clearAutoReconnectTimer();
      // Load silenced channels for this server (pendingConnect still available).
      const pending = useAppStore.getState().pendingConnect;
      let silenced = new Set<number>();
      let mutedPush = new Set<number>();
      if (pending) {
        const serverKey = `${pending.host}:${pending.port}`;
        const ids = await getSilencedChannels(serverKey);
        silenced = new Set(ids);
        const mutedIds = await getMutedPushChannels(serverKey);
        mutedPush = new Set(mutedIds);
      }

      // Load persisted per-user volumes and reset the applied-session tracker.
      volumeAppliedSessions.clear();
      let storedVolumes: Record<string, number> = {};
      try {
        storedVolumes = await getUserVolumes();
      } catch {
        // Store may not be ready yet.
      }

      // Navigate immediately - don't block on data fetching.
      useAppStore.setState({
        status: "connected",
        passwordRequired: false,
        silencedChannels: silenced,
        mutedPushChannels: mutedPush,
        userVolumes: storedVolumes,
        bootstrapStage: "Fetching channels and users...",
      });

      // Refresh the multi-server session list so any newly-connected
      // server appears in the sessions slice immediately.
      useAppStore.getState().refreshSessions().catch(() => {
        // best-effort; the sessions list will be repopulated on next event.
      }).then(() => {
        // Clear any stale per-session error stored from a prior disconnect
        // for this newly-connected session.
        const { activeServerId } = useAppStore.getState();
        if (activeServerId) {
          useAppStore.setState((prev) => {
            if (prev.sessionErrors[activeServerId] == null) return prev;
            const next = { ...prev.sessionErrors };
            delete next[activeServerId];
            return { sessionErrors: next };
          });
        }
      });

      // Load channels/users/messages, then resolve identity, then
      // hand off to the chat view.  We delay `navigate("/chat")` until
      // the visible bootstrap is done so the connect-page progress bar
      // stays visible until the chat view actually has data.
      useAppStore
        .getState()
        .refreshState()
        .then(async () => {
          // Fetch the channel the user is currently in.
          useAppStore.setState({ bootstrapStage: "Locating your channel..." });
          const currentCh = await invoke<number | null>("get_current_channel");
          if (currentCh !== null) {
            useAppStore.setState({ currentChannel: currentCh });
          }

          // Fetch our own session ID.
          useAppStore.setState({ bootstrapStage: "Identifying you to the server..." });
          const ownSession = await invoke<number | null>("get_own_session");
          useAppStore.setState({ ownSession });

          // Fetch the server's Fancy Mumble version (null for standard servers).
          try {
            const info = await invoke<ServerInfo>("get_server_info");
            useAppStore.setState({ serverFancyVersion: info.fancy_version });
          } catch {
            // Server info unavailable - leave as null.
          }

          // Auto-apply the locally saved profile for unregistered users.
          // Registered users have their profile stored server-side, but
          // unregistered users lose it on each connect.
          if (ownSession !== null) {
            const ownUser = useAppStore.getState().users.find((u) => u.session === ownSession);
            const isRegistered = ownUser?.user_id != null && ownUser.user_id > 0;
            if (!isRegistered) {
              const identityLabel = useAppStore.getState().connectedCertLabel ?? null;
              loadProfileData(identityLabel)
                .then(async ({ profile, bio, avatarDataUrl }) => {
                  const comment = serializeProfile(profile, bio);
                  if (comment) {
                    await invoke("set_user_comment", { comment });
                  }
                  const texture = avatarDataUrl ? dataUrlToBytes(avatarDataUrl) : [];
                  if (texture.length > 0) {
                    await invoke("set_user_texture", { texture });
                  }
                })
                .catch((err) => console.error("Auto-apply profile error:", err));
            }
          }

          const { channels, selectedChannel } = useAppStore.getState();
          if (selectedChannel === null && channels.length > 0) {
            // Default to the channel the user is in, falling back to the first channel.
            const defaultCh = currentCh ?? channels[0].id;
            useAppStore.getState().selectChannel(defaultCh);
          }

          // Apply persisted per-user volumes to backend for all current users.
          applyStoredVolumesToNewUsers();

          // Visible bootstrap is done - drop the loading bar and reveal the chat view.
          useAppStore.setState({ bootstrapStage: null });
          navigate("/chat");

          // Restore voice call state from before the disconnect.
          // isRestoringVoice suppresses pref writes from the
          // voice-state-changed listener during this sequence so that
          // rapid "active" then "muted" events cannot race and
          // clobber voiceMutedOnReconnect with false.
          try {
            const prefs = await getPreferences();
            if (prefs.voiceOnReconnect) {
              isRestoringVoice = true;
              try {
                await useAppStore.getState().enableVoice();
                if (prefs.voiceMutedOnReconnect) {
                  await useAppStore.getState().toggleMute();
                }
              } finally {
                isRestoringVoice = false;
              }
            }
          } catch {
            // Voice restore is best-effort.
          }
        })
        .catch((err) => {
          // If the bootstrap chain fails, surface it but never leave the UI
          // stranded on a permanent loading bar.
          console.error("Post-connect bootstrap error:", err);
          useAppStore.setState({ bootstrapStage: null });
          navigate("/chat");
        });
    }),
  );

  // Connection dropped.
  unlisteners.push(
    await listen<{ serverId?: string | null; reason: string | null } | string | null>(
      "server-disconnected",
      async (event) => {
        // Normalise: backend now always sends an object payload, but tolerate
        // a bare reason string for forwards/backwards compatibility.
        const payload = event.payload;
        const eventServerId = typeof payload === "object" && payload !== null
          ? (payload.serverId ?? null)
          : null;
        const eventReason = typeof payload === "string"
          ? payload
          : (typeof payload === "object" && payload !== null ? payload.reason : null);

        const { activeServerId, pendingConnect: pendingForActive } = useAppStore.getState();
        // Only treat the event as affecting the active session if the
        // backend explicitly tagged it with the active session's id.
        // A missing/null serverId means "unknown" - we must not assume
        // it belongs to the currently-focused tab, otherwise closing a
        // background session would clobber the foreground one.
        //
        // Exception: if a `pendingConnect` is in flight AND we don't yet
        // have an active session (initial connect race - the backend
        // fired the disconnect before our `refreshSessions()` returned),
        // fall back to assuming the event belongs to the pending connect
        // so the user sees the error instead of being stuck on a
        // "connecting" skeleton. Crucially, we do NOT apply this fallback
        // when the user already has an active session AND the
        // `eventServerId` either differs from it or is unknown - in that
        // case the disconnect belongs to a *different* tab (a new
        // connection attempt) and clobbering the active one would make
        // the foreground server's tab unusable.
        const pendingFallbackApplies =
          pendingForActive !== null &&
          activeServerId === null &&
          (eventServerId === null || eventServerId !== activeServerId);
        const isActiveSession =
          (eventServerId !== null && eventServerId === activeServerId) ||
          pendingFallbackApplies;

        // If the user explicitly closed this session via the tab close
        // button, the `disconnectSession` action manages the UI handoff
        // (refresh + switch to next active tab).  Skip the listener's
        // own state-clobbering cleanup so we don't flash a misleading
        // "Connection lost" overlay on the *next* tab.
        if (eventServerId && intentionallyClosingSessions.has(eventServerId)) {
          await useAppStore.getState().refreshSessions().catch(() => {});
          return;
        }

        // Always refresh the sessions list so the disconnected tab updates
        // its status dot / badge regardless of which tab was affected.
        await useAppStore.getState().refreshSessions().catch(() => {});

        // Always remember the disconnect reason for this specific session
        // so the user sees the correct reason when they switch tabs.
        // Skip overwriting an already-stored reason with null - kick events
        // may be followed by a generic on_disconnected with no reason.
        if (eventServerId && eventReason) {
          useAppStore.setState((prev) => ({
            sessionErrors: { ...prev.sessionErrors, [eventServerId]: eventReason },
          }));
        }

        if (!isActiveSession) {
          // A non-active session disconnected: do not touch the active
          // session's state (status, channels, users, etc.).  The tab
          // bar already reflects the new status; nothing else to do.
          return;
        }

        // Active session was the one that disconnected — proceed with
        // the full local cleanup.
        offloadManager.dispose().catch(() => {});
        volumeAppliedSessions.clear();
        clearReadReceipts();
        const { error: currentError, passwordRequired: pwRequired, pendingConnect: pending } = useAppStore.getState();
        // If a password prompt is already pending, keep the rejection error
        // instead of overwriting it with a generic disconnect message.
        const reason = pwRequired ? currentError : (eventReason ?? currentError);
        useAppStore.setState({ ...INITIAL, error: reason, passwordRequired: pwRequired, pendingConnect: pending });
        invoke("update_badge_count", { count: null }).catch(() => {});

        const { sessions } = useAppStore.getState();
        if (sessions.length === 0 || pwRequired) {
          navigate("/");
        } else {
          navigate("/chat");
        }

        if (!manualDisconnectRequested && !pwRequired && pending) {
          scheduleAutoReconnect(pending);
        }
      },
    ),
  );

  // Channel / user list changed - debounce rapid-fire updates.
  let stateChangeTimer: ReturnType<typeof setTimeout> | undefined;
  unlisteners.push(
    await listen("state-changed", () => {
      clearTimeout(stateChangeTimer);
      stateChangeTimer = setTimeout(() => {
        useAppStore
          .getState()
          .refreshState()
          .then(() => applyStoredVolumesToNewUsers());
      }, 100);
    }),
  );

  // Messages, unreads, groups, connection events.
  unlisteners.push(
    // Server activity log entry.
    await listen<ServerLogEntry>("server-log", (event) => {
      const MAX_LOG_ENTRIES = 200;
      useAppStore.setState((prev) => {
        const log = [...prev.serverLog, event.payload];
        if (log.length > MAX_LOG_ENTRIES) {
          log.splice(0, log.length - MAX_LOG_ENTRIES);
        }
        return { serverLog: log };
      });
    }),

    // New text message arrived.
    await listen<{ channel_id: number; sender_session: number | null }>("new-message", async (event) => {
      const { channel_id, sender_session } = event.payload;

      // Clear the sender's typing indicator immediately.
      if (sender_session != null) {
        useAppStore.setState((prev) => {
          const channelSet = prev.typingUsers.get(channel_id);
          if (!channelSet?.has(sender_session)) return prev;
          const next = new Map(prev.typingUsers);
          const updated = new Set(channelSet);
          updated.delete(sender_session);
          if (updated.size === 0) {
            next.delete(channel_id);
          } else {
            next.set(channel_id, updated);
          }
          return { typingUsers: next };
        });
      }

      const { selectedChannel } = useAppStore.getState();
      if (selectedChannel === channel_id) {
        await useAppStore.getState().refreshMessages(channel_id);
      }
    }),

    // New direct message arrived.
    await listen<{ session: number }>("new-dm", async (event) => {
      const { selectedDmUser } = useAppStore.getState();
      if (selectedDmUser === event.payload.session) {
        await useAppStore
          .getState()
          .refreshDmMessages(event.payload.session);
      }
    }),

    // Unread counts changed.
    await listen<{ unreads: Record<number, number>; serverId?: string | null }>(
      "unread-changed",
      (event) => {
        const { activeServerId } = useAppStore.getState();
        const eventServerId = event.payload.serverId ?? null;
        // Compute total for this session (sum of unreads).
        const total = Object.values(event.payload.unreads).reduce((a, b) => a + b, 0);
        if (eventServerId && eventServerId !== activeServerId) {
          // Non-active session: only update its per-tab badge total.
          useAppStore.setState((prev) => {
            // Combine channel total with whatever DM total we last saw
            // for this session (we store the channel total alone here;
            // dm-unread updates merge in the same way).
            const next = { ...prev.sessionUnreadTotals };
            const prevDm = next[`${eventServerId}:dm`] ?? 0;
            next[`${eventServerId}:ch`] = total;
            next[eventServerId] = total + prevDm;
            return { sessionUnreadTotals: next };
          });
          return;
        }
        useAppStore.setState({ unreadCounts: event.payload.unreads });
        updateBadgeCount();
      },
    ),

    // DM unread counts changed.
    await listen<{ unreads: Record<number, number>; serverId?: string | null }>(
      "dm-unread-changed",
      (event) => {
        const { activeServerId } = useAppStore.getState();
        const eventServerId = event.payload.serverId ?? null;
        const total = Object.values(event.payload.unreads).reduce((a, b) => a + b, 0);
        if (eventServerId && eventServerId !== activeServerId) {
          useAppStore.setState((prev) => {
            const next = { ...prev.sessionUnreadTotals };
            const prevCh = next[`${eventServerId}:ch`] ?? 0;
            next[`${eventServerId}:dm`] = total;
            next[eventServerId] = total + prevCh;
            return { sessionUnreadTotals: next };
          });
          return;
        }
        useAppStore.setState({ dmUnreadCounts: event.payload.unreads });
        updateBadgeCount();
      },
    ),

    // Server rejected the connection.
    await listen<{ serverId?: string | null; reason: string; reject_type: number | null }>("connection-rejected", async (event) => {
      // Always remember the rejection reason for this session so the
      // user sees it when they switch to its tab.
      const eventServerId = event.payload.serverId ?? null;
      if (eventServerId) {
        useAppStore.setState((prev) => ({
          sessionErrors: { ...prev.sessionErrors, [eventServerId]: event.payload.reason },
        }));
      }
      // Ignore rejections targeting non-active sessions: the matching
      // server-disconnected event will surface them via the per-session
      // status and the reconnect overlay when the user opens that tab.
      //
      // Exception: if a `pendingConnect` is in flight AND we have no
      // active session yet (initial connect race - backend fired the
      // rejection before `refreshSessions()` returned), fall back to
      // assuming the rejection belongs to the pending connect so the
      // user sees the error instead of being stuck on a "connecting"
      // skeleton. We do NOT apply this fallback when the user already
      // has an active session AND `eventServerId` differs - that would
      // clobber the foreground server's tab when a *different* tab's
      // connection attempt was rejected.
      const { activeServerId, pendingConnect } = useAppStore.getState();
      const pendingFallbackApplies =
        pendingConnect !== null &&
        activeServerId === null &&
        (eventServerId === null || eventServerId !== activeServerId);
      if (eventServerId !== null && eventServerId !== activeServerId && !pendingFallbackApplies) {
        return;
      }
      const rt = event.payload.reject_type;
      // WrongUserPW = 3, WrongServerPW = 4
      const isPasswordError = rt === 3 || rt === 4;
      if (isPasswordError) {
        const { passwordAttempted } = useAppStore.getState();
        useAppStore.setState({
          status: "disconnected",
          error: passwordAttempted ? event.payload.reason : null,
          passwordRequired: true,
          bootstrapStage: null,
          // pendingConnect was set by the connect action - keep it
          // so the dialog can re-issue the connect with the password.
        });
        // Make sure the failed-session tab is the active one so the
        // PasswordDialog (rendered on /chat over the disconnected
        // session card) appears anchored to it.  Otherwise the user
        // is left on "/" which renders an extra synthetic "New
        // connection" tab alongside the real failed-session tab.
        await useAppStore.getState().refreshSessions().catch(() => {});
        const { sessions } = useAppStore.getState();
        if (eventServerId && sessions.some((s) => s.id === eventServerId)) {
          await useAppStore.getState().switchServer(eventServerId).catch(() => {});
          // switchServer re-syncs status/error from the session
          // metadata, which would clobber passwordRequired.  Restore.
          useAppStore.setState({
            status: "disconnected",
            passwordRequired: true,
            error: passwordAttempted ? event.payload.reason : null,
            pendingConnect: useAppStore.getState().pendingConnect,
          });
          navigate("/chat");
        }
        return;
      }
      useAppStore.setState({
        status: "disconnected",
        error: event.payload.reason,
        pendingConnect: null,
        bootstrapStage: null,
      });
      // Stay on /chat when other tabs remain so the reconnect overlay
      // surfaces the kick/ban reason via `error`.  Connect-time failures
      // (no other sessions) fall back to the connect page.
      await useAppStore.getState().refreshSessions().catch(() => {});
      const { sessions } = useAppStore.getState();
      navigate(sessions.length > 0 ? "/chat" : "/");
    }),

    // Listen request was denied by the server - revert the UI.
    await listen<{ channel_id: number }>("listen-denied", (event) => {
      useAppStore.setState((prev) => {
        const next = new Set(prev.listenedChannels);
        next.delete(event.payload.channel_id);
        return { listenedChannels: next };
      });
    }),

    // Our own user moved to a different channel.
    await listen<{ channel_id: number }>("current-channel-changed", (event) => {
      useAppStore.setState({ currentChannel: event.payload.channel_id });
    }),

    // User tapped a chat notification - navigate to the target channel.
    await listen<{ channel_id: number }>("navigate-to-channel", (event) => {
      const channelId = event.payload.channel_id;
      navigate("/chat");
      useAppStore.getState().selectChannel(channelId);
    }),

    // Voice state changed (enable/disable voice calling).
    // Pref writes are NOT done here: queued IPC messages can arrive in the
    // wrong order (especially with a slow backend event loop) and corrupt
    // voiceMutedOnReconnect. Prefs are written by the explicit action
    // handlers (enableVoice, disableVoice, toggleMute) where ordering is
    // deterministic relative to the user's intent.
    await listen<VoiceState>("voice-state-changed", (event) => {
      const updates: Partial<ReturnType<typeof useAppStore.getState>> = { voiceState: event.payload };
      if (event.payload === "inactive") {
        updates.talkingSessions = new Set();
      }
      useAppStore.setState(updates);
    }),

    // Audio transport mode changed (UDP vs TCP tunnel).
    await listen<boolean>("audio-transport-changed", (event) => {
      useAppStore.setState({ udpActive: event.payload });
    }),

    // User talking state changed (audio transmission start/stop).
    await listen<[number, boolean]>("user-talking", (event) => {
      const [session, talking] = event.payload;
      const prev = useAppStore.getState().talkingSessions;
      const next = new Set(prev);
      if (talking) {
        next.add(session);
      } else {
        next.delete(session);
      }
      useAppStore.setState({ talkingSessions: next });
    }),

    // Server config received (limits, allow_html, etc.).
    await listen("server-config", async () => {
      try {
        const cfg = await invoke<MumbleServerConfig>("get_server_config");
        useAppStore.setState((state) => {
          const next: { serverConfig: MumbleServerConfig; fileServerConfig?: FileServerConfig } = { serverConfig: cfg };
          const override = cfg.fancy_rest_api_url;
          if (override && override.length > 0 && state.fileServerConfig) {
            next.fileServerConfig = { ...state.fileServerConfig, baseUrl: override.replace(/\/+$/, "") };
          }
          return next;
        });
        void probeFileServerCapabilities();
      } catch (e) {
        console.error("get_server_config error:", e);
      }
    }),
  );

  // Plugin data received (polls, etc.).
  // Process polls and votes directly here so the data reaches the
  // Zustand store even across Vite HMR reloads and React StrictMode
  // double-mounts where the old handler-array dispatch could fail.
  unlisteners.push(
    await listen<{ sender_session: number | null; data: number[]; data_id: string }>(
      "plugin-data",
      (event) => {
        const { data_id, data, sender_session } = event.payload;
        const bytes = new Uint8Array(data);

        if (data_id === "fancy-file-server-config") {
          try {
            const json = new TextDecoder().decode(bytes);
            const raw = JSON.parse(json);
            // The server-wide override (advertised in `ServerConfig`)
            // takes precedence over the per-plugin `base_url`. This
            // matters when the HTTP interface is hosted behind a
            // reverse proxy or ingress and reachable at a different
            // hostname than the Mumble TCP port.
            const override = useAppStore.getState().serverConfig.fancy_rest_api_url;
            const baseUrl = (override && override.length > 0) ? override : raw.base_url;
            const cfg: FileServerConfig = {
              baseUrl,
              sessionId: raw.session_id,
              uploadToken: raw.upload_token,
              sessionJwt: raw.session_jwt,
              maxFileSizeBytes: raw.max_file_size_bytes,
              deleteOnTtl: !!raw.delete_on_ttl,
              ttlSeconds: raw.ttl_seconds ?? 0,
              deleteOnDownload: !!raw.delete_on_download,
              deleteOnDisconnect: !!raw.delete_on_disconnect,
              canManageEmotes: !!raw.can_manage_emotes,
              canShareFiles: raw.can_share_files !== false,
              canShareFilesPublic: raw.can_share_files_public !== false,
            };
            useAppStore.setState({ fileServerConfig: cfg });
          } catch (e) {
            console.error("plugin-data file-server-config processing error:", e);
          }
        }

        if (data_id === "fancy-server-emotes") {
          try {
            const json = new TextDecoder().decode(bytes);
            const raw = JSON.parse(json) as { emotes: Array<{
              shortcode: string;
              alias_emoji: string;
              description?: string;
              image_data_url: string;
            }> };
            const emotes: CustomServerEmote[] = (raw.emotes ?? []).map((e) => ({
              shortcode: e.shortcode,
              aliasEmoji: e.alias_emoji,
              description: e.description,
              imageDataUrl: e.image_data_url,
            }));
            useAppStore.setState({ customServerEmotes: emotes });
            const reactions: ServerCustomReaction[] = emotes.map((e) => ({
              shortcode: `:${e.shortcode}:`,
              display: e.imageDataUrl,
              label: e.description ?? e.aliasEmoji,
            }));
            setServerCustomReactions(reactions);
          } catch (e) {
            console.error("plugin-data server-emotes processing error:", e);
          }
        }

        if (data_id === "fancy-poll" || data_id === "fancy-poll-vote") {
          try {
            const json = new TextDecoder().decode(bytes);
            const payload = JSON.parse(json);

            if (data_id === "fancy-poll" && payload.type === "poll") {
              const poll = payload as PollPayload;
              poll.creator = sender_session ?? poll.creator;
              // Resolve creator name from current users.
              const users = useAppStore.getState().users;
              const user = users.find((u) => u.session === poll.creator);
              if (user) poll.creatorName = user.name;
              useAppStore.getState().addPoll(poll, false);
            } else if (data_id === "fancy-poll-vote" && payload.type === "poll_vote") {
              const vote = payload as PollVotePayload;
              vote.voter = sender_session ?? vote.voter;
              const users = useAppStore.getState().users;
              const user = users.find((u) => u.session === vote.voter);
              if (user) vote.voterName = user.name;
              registerVote(vote);
              // Trigger re-render for any component reading polls.
              useAppStore.setState({});
            }
          } catch (e) {
            console.error("plugin-data poll processing error:", e);
          }
        }

        // Also dispatch to legacy registered handlers for extensibility.
        for (const handler of pluginDataHandlers) {
          handler(data_id, bytes, sender_session);
        }
      },
    ),
  );

  // -- WebRTC signal events ----------------------------------------

  unlisteners.push(
    await listen<{ sender_session: number | null; target_session: number | null; signal_type: number; payload: string; serverId?: string | null }>(
      "webrtc-signal",
      (event) => {
        const { sender_session, target_session, signal_type, payload } = event.payload;
        const serverId = event.payload.serverId ?? null;
        for (const handler of webRtcSignalHandlers) {
          handler(sender_session, target_session, signal_type, payload, serverId);
        }
      },
    ),
  );

  // -- Link preview response events --------------------------------

  unlisteners.push(
    await listen<{ request_id: string; embeds: import("./types").LinkEmbed[] }>(
      "link-preview-response",
      (event) => {
        const { request_id, embeds } = event.payload;
        if (!request_id || !Array.isArray(embeds) || embeds.length === 0) return;
        const prev = useAppStore.getState().linkEmbeds;
        const next = new Map(prev);
        next.set(request_id, embeds);
        useAppStore.setState({ linkEmbeds: next });
      },
    ),
  );

  // -- Custom reactions config event --------------------------------

  unlisteners.push(
    await listen<ServerCustomReaction[]>(
      "custom-reactions-config",
      (event) => {
        const reactions = event.payload;
        if (Array.isArray(reactions)) {
          setServerCustomReactions(reactions);
        }
      },
    ),
  );

  // -- Read receipt events -----------------------------------------

  unlisteners.push(
    await listen<ReadReceiptDeliverPayload>(
      "read-receipt-deliver",
      (event) => {
        const { channel_id, read_states } = event.payload;
        applyReadStates(channel_id, read_states);
        useAppStore.setState((prev) => ({
          readReceiptVersion: prev.readReceiptVersion + 1,
        }));
      },
    ),
  );

  // -- Typing indicator events ------------------------------------

  unlisteners.push(
    await listen<{ session: number; channel_id: number }>(
      "typing-indicator",
      (event) => {
        const { session, channel_id } = event.payload;
        useAppStore.setState((prev) => {
          const next = new Map(prev.typingUsers);
          const channelSet = new Set(next.get(channel_id));
          channelSet.add(session);
          next.set(channel_id, channelSet);
          return { typingUsers: next };
        });

        // Auto-expire after 5 seconds.
        setTimeout(() => {
          useAppStore.setState((prev) => {
            const next = new Map(prev.typingUsers);
            const channelSet = next.get(channel_id);
            if (!channelSet) return prev;
            const updated = new Set(channelSet);
            updated.delete(session);
            if (updated.size === 0) {
              next.delete(channel_id);
            } else {
              next.set(channel_id, updated);
            }
            return { typingUsers: next };
          });
        }, 5000);
      },
    ),
  );

  // -- Watch-together (FancyWatchSync) events ---------------------

  unlisteners.push(
    await listen<WatchSyncPayload>("watch-sync", (event) => {
      applyWatchSyncEvent(event.payload);
    }),
  );

  // -- Persistent chat events -------------------------------------

  unlisteners.push(
    // Channel persistence config changed (from ChannelState updates).
    await listen<{ channel_id: number; config: ChannelPersistConfig }>(
      "persistence-config-changed",
      (event) => {
        const { channel_id, config } = event.payload;
        useAppStore.getState().updateChannelPersistenceConfig(channel_id, config);
      },
    ),

    // Key trust level changed for a channel.
    await listen<{ channel_id: number; trust: KeyTrustState }>(
      "key-trust-changed",
      (event) => {
        const { channel_id, trust } = event.payload;
        useAppStore.setState((prev) => {
          // Receiving a new key clears the revoked flag for this channel.
          const next = new Set(prev.pchatKeyRevoked);
          next.delete(channel_id);
          return {
            keyTrust: { ...prev.keyTrust, [channel_id]: trust },
            pchatKeyRevoked: next,
          };
        });
      },
    ),

    // Custodian list changed (TOFU change detection).
    await listen<{ channel_id: number; pin: CustodianPinState }>(
      "custodian-pin-changed",
      (event) => {
        const { channel_id, pin } = event.payload;
        useAppStore.setState((prev) => ({
          custodianPins: { ...prev.custodianPins, [channel_id]: pin },
        }));
      },
    ),

    // Key dispute detected.
    await listen<{ channel_id: number; dispute: PendingDispute }>(
      "key-dispute-detected",
      (event) => {
        const { channel_id, dispute } = event.payload;
        useAppStore.setState((prev) => ({
          pendingDisputes: { ...prev.pendingDisputes, [channel_id]: dispute },
        }));
      },
    ),

    // Key dispute resolved (by custodian shortcut or timeout).
    await listen<{ channel_id: number }>(
      "key-dispute-resolved",
      (event) => {
        const { channel_id } = event.payload;
        useAppStore.setState((prev) => {
          const { [channel_id]: _removed, ...rest } = prev.pendingDisputes;
          return { pendingDisputes: rest };
        });
      },
    ),

    // Pchat history loading state (waiting for key exchange).
    await listen<{ channel_id: number; loading: boolean }>(
      "pchat-history-loading",
      (event) => {
        const { channel_id, loading } = event.payload;
        const next = new Set(useAppStore.getState().pchatHistoryLoading);
        if (loading) {
          next.add(channel_id);
        } else {
          next.delete(channel_id);
        }
        useAppStore.setState({ pchatHistoryLoading: next });
      },
    ),

    // Pchat fetch complete -- update pagination metadata.
    await listen<{ channel_id: number; has_more: boolean; total_stored: number }>(
      "pchat-fetch-complete",
      (event) => {
        const { channel_id, has_more, total_stored } = event.payload;
        useAppStore.setState((prev) => ({
          channelPersistence: {
            ...prev.channelPersistence,
            [channel_id]: {
              ...prev.channelPersistence[channel_id],
              hasMore: has_more,
              isFetching: false,
              totalStored: total_stored,
            },
          },
        }));
      },
    ),

    // A new key-share consent request from the backend.
    await listen<PendingKeyShareRequest>(
      "pchat-key-share-request",
      (event) => {
        const req = event.payload;
        useAppStore.setState((prev) => {
          const existing = prev.pendingKeyShares[req.channel_id] ?? [];
          // Avoid duplicates.
          if (existing.some((p) => p.peer_cert_hash === req.peer_cert_hash)) {
            return {};
          }
          return {
            pendingKeyShares: {
              ...prev.pendingKeyShares,
              [req.channel_id]: [...existing, req],
            },
          };
        });
      },
    ),

    // Key-share requests changed (after approve/dismiss).
    await listen<{ channel_id: number; pending: PendingKeyShareRequest[] }>(
      "pchat-key-share-requests-changed",
      (event) => {
        const { channel_id, pending } = event.payload;
        useAppStore.setState((prev) => {
          if (pending.length === 0) {
            const { [channel_id]: _removed, ...rest } = prev.pendingKeyShares;
            return { pendingKeyShares: rest };
          }
          return {
            pendingKeyShares: {
              ...prev.pendingKeyShares,
              [channel_id]: pending,
            },
          };
        });
      },
    ),

    // Key holders list updated by the server.
    await listen<{ channel_id: number; holders: KeyHolderEntry[] }>(
      "pchat-key-holders-changed",
      (event) => {
        const { channel_id, holders } = event.payload;
        useAppStore.setState((prev) => ({
          keyHolders: {
            ...prev.keyHolders,
            [channel_id]: holders,
          },
        }));
      },
    ),

    // Key restored: a new key was received after a previous revocation.
    await listen<{ channel_id: number }>(
      "pchat-key-restored",
      (event) => {
        const { channel_id } = event.payload;
        useAppStore.setState((prev) => {
          const next = new Set(prev.pchatKeyRevoked);
          next.delete(channel_id);
          return { pchatKeyRevoked: next };
        });
      },
    ),

    // Key-possession challenge failed: our key was wrong/outdated.
    await listen<{ channel_id: number }>(
      "pchat-key-revoked",
      (event) => {
        const { channel_id } = event.payload;
        useAppStore.setState((prev) => {
          const next = new Set(prev.pchatKeyRevoked);
          next.add(channel_id);
          // Clear stale key-trust for this channel.
          const { [channel_id]: _removedTrust, ...restTrust } = prev.keyTrust;
          // Clear any messages that were decrypted before the challenge
          // result arrived (prevents flash of unauthorized content).
          const clearMessages = prev.selectedChannel === channel_id;
          // Stop the loading spinner — no fetch response will arrive.
          const nextLoading = new Set(prev.pchatHistoryLoading);
          nextLoading.delete(channel_id);
          const { [channel_id]: prevPersist, ...restPersist } = prev.channelPersistence;
          return {
            pchatKeyRevoked: next,
            keyTrust: restTrust,
            pchatHistoryLoading: nextLoading,
            channelPersistence: {
              ...restPersist,
              [channel_id]: { ...prevPersist, isFetching: false },
            },
            ...(clearMessages ? { messages: [] } : {}),
          };
        });
      },
    ),

    // Reaction add/remove delivered by the server (persistent channels).
    await listen<ReactionDeliverEvent>(
      "pchat-reaction-deliver",
      (event) => {
        const { message_id, emoji, action, sender_hash, sender_name } = event.payload;
        const resolvedName = useAppStore.getState().users.find((u) => u.hash === sender_hash)?.name ?? sender_name;
        applyReaction(message_id, emoji, action as "add" | "remove", sender_hash, resolvedName);
        useAppStore.setState((s) => ({ reactionVersion: s.reactionVersion + 1 }));
      },
    ),

    // Batch reaction fetch response (historical reactions for persistent channels).
    await listen<ReactionFetchResponseEvent>(
      "pchat-reaction-fetch-response",
      (event) => {
        const { users } = useAppStore.getState();
        for (const r of event.payload.reactions) {
          const resolvedName = users.find((u) => u.hash === r.sender_hash)?.name ?? r.sender_name;
          applyReaction(r.message_id, r.emoji, "add", r.sender_hash, resolvedName);
        }
        useAppStore.setState((s) => ({ reactionVersion: s.reactionVersion + 1 }));
      },
    ),

    // Pin/unpin delivered by the server (persistent channels).
    await listen<PinDeliverEvent>(
      "pchat-pin-deliver",
      (event) => {
        const { channel_id, message_id, pinned, pinner_hash, pinner_name, timestamp } = event.payload;
        const resolvedName = useAppStore.getState().users.find((u) => u.hash === pinner_hash)?.name ?? pinner_name;
        useAppStore.setState((s) => {
          const nextUnseen = new Map(s.unseenPinIds);
          const channelSet = new Set(nextUnseen.get(channel_id));
          if (pinned) {
            channelSet.add(message_id);
          } else {
            channelSet.delete(message_id);
          }
          if (channelSet.size > 0) nextUnseen.set(channel_id, channelSet);
          else nextUnseen.delete(channel_id);

          return {
            messages: s.messages.map((m) =>
              m.message_id === message_id
                ? { ...m, pinned, pinned_by: pinned ? resolvedName : null, pinned_at: pinned ? timestamp : null }
                : m,
            ),
            unseenPinIds: nextUnseen,
          };
        });
      },
    ),

    // Batch pin fetch response (historical pins for persistent channels).
    await listen<PinFetchResponseEvent>(
      "pchat-pin-fetch-response",
      (event) => {
        const { users } = useAppStore.getState();
        const pinnedIds = new Map(event.payload.pins.map((p) => {
          const resolvedName = users.find((u) => u.hash === p.pinner_hash)?.name ?? p.pinner_name;
          return [p.message_id, { pinned_by: resolvedName, pinned_at: p.timestamp }] as const;
        }));
        useAppStore.setState((s) => ({
          messages: s.messages.map((m) => {
            const pin = m.message_id ? pinnedIds.get(m.message_id) : undefined;
            return pin ? { ...m, pinned: true, pinned_by: pin.pinned_by, pinned_at: pin.pinned_at } : m;
          }),
        }));
      },
    ),

    // Signal bridge load failure: show error banner in the UI.
    await listen<{ message: string }>(
      "pchat-signal-bridge-error",
      (event) => {
        useAppStore.setState({ signalBridgeError: event.payload.message });
      },
    ),
  );

  return unlisteners;
}
