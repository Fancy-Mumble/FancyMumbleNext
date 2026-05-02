/**
 * Player adapter interface.
 *
 * A player adapter is the glue between the watch-together state
 * machine (which knows about play/pause/seek and a host's
 * authoritative state) and a concrete underlying player implementation
 * (HTML5 `<video>`, YouTube IFrame, ...).
 *
 * Adapters are mounted into a host DOM element by the
 * `WatchTogetherCard` component and report local user-initiated
 * playback events back via `onLocalEvent`.  They are explicitly
 * designed to be pure DOM (no React) so that the host component can
 * remain lightweight and the adapter can be swapped without remount.
 */

/** Local event the adapter reports back to the controller. */
export interface LocalPlayerEvent {
  /** Current playback state inferred from the underlying player. */
  state: "playing" | "paused" | "ended";
  /** Position in seconds. */
  currentTime: number;
}

/** Constructor arguments shared by every adapter. */
export interface PlayerAdapterArgs {
  /** DOM container the adapter mounts its <video>/iframe into. */
  container: HTMLElement;
  /** Source URL (direct media or canonical YouTube watch URL). */
  sourceUrl: string;
  /** Notification callback for any locally-originated event (host only). */
  onLocalEvent?: (event: LocalPlayerEvent) => void;
}

/** Common adapter API. */
export interface PlayerAdapter {
  /** Begin playback at the given position. */
  play(at: number): Promise<void>;
  /** Pause playback at the given position. */
  pause(at: number): Promise<void>;
  /** Seek to the given position without changing play/pause state. */
  seek(at: number): Promise<void>;
  /** Read the current playback position in seconds. */
  currentTime(): number;
  /** Replace (or clear) the local-event callback after construction. */
  setOnLocalEvent(cb: ((event: LocalPlayerEvent) => void) | undefined): void;
  /** Tear down the underlying player and remove DOM nodes. */
  destroy(): void;
}
