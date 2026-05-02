/**
 * Watch-together (FancyWatchSync) wire types.
 *
 * Mirrors the JSON shape produced by the Rust handler in
 * `crates/mumble-tauri/src/state/handler/watch_sync.rs` and accepted by
 * the `send_watch_sync` Tauri command.  Keep in sync with both.
 */

/** Recognised media source kinds. */
export type WatchSourceKind = "directMedia" | "youtube";

/** Playback state mirroring `mumble_tcp::fancy_watch_sync::PlaybackState`. */
export type WatchPlaybackState = "paused" | "playing" | "ended";

/** Tagged union of watch-sync events.  Each variant maps 1:1 to a proto oneof arm. */
export type WatchSyncEvent =
  | {
      type: "start";
      channelId?: number;
      sourceUrl?: string;
      sourceKind?: WatchSourceKind;
      title?: string;
      hostSession?: number;
    }
  | {
      type: "state";
      state?: WatchPlaybackState;
      currentTime?: number;
      updatedAtMs?: number;
      hostSession?: number;
    }
  | { type: "join"; session?: number }
  | { type: "leave"; session?: number }
  | { type: "stateRequest" }
  | { type: "end" }
  | { type: "hostTransfer"; newHostSession?: number };

/**
 * Inbound payload emitted by the Rust handler as the `"watch-sync"`
 * Tauri event.
 */
export interface WatchSyncPayload {
  /** Watch session UUID (chosen by the originator). */
  sessionId?: string;
  /** Session ID of the user that emitted this event (filled by the server). */
  actor?: number;
  event: WatchSyncEvent;
}

/**
 * Local in-memory representation of a watch-together session.
 *
 * Built up from incoming `WatchSyncPayload`s and used by the UI to
 * render the player card.
 */
export interface WatchSession {
  sessionId: string;
  channelId: number;
  hostSession: number;
  sourceUrl: string;
  sourceKind: WatchSourceKind;
  title?: string;
  /** Set of session IDs currently watching (includes host). */
  participants: Set<number>;
  /** Last known playback state from the host. */
  state: WatchPlaybackState;
  /** Last known playback position in seconds. */
  currentTime: number;
  /** Sender's wall-clock time (ms) attached to the last state update. */
  updatedAtMs: number;
}
