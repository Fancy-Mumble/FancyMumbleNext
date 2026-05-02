/**
 * Watch-together controller hook.
 *
 * Wires a `PlayerAdapter` to the local `WatchSession` store and the
 * `send_watch_sync` Tauri command:
 *
 * - When the local user is host, forward `LocalPlayerEvent`s to the
 *   wire as `state` events (rate-limited to ~1 Hz).
 * - When a remote `state` arrives, apply it to the adapter if the
 *   playback drift exceeds 0.5 s.  Drift larger than 60 s pauses the
 *   adapter and surfaces an "out of sync" flag the UI can render.
 *
 * The hook owns no DOM; the component must pass in a ready adapter
 * (typically constructed via `createPlayerAdapter`).
 */

import { useCallback, useEffect, useRef, useState } from "react";

import type { LocalPlayerEvent, PlayerAdapter } from "./PlayerAdapter";
import { useWatchSend } from "./useWatchSend";
import type { WatchPlaybackState, WatchSession } from "./watchTypes";

/** Drift in seconds at which we re-sync the local adapter. */
const DRIFT_RESYNC_SECONDS = 0.5;
/**
 * Minimum interval between host->wire heartbeats *for the same
 * playback state and position*.  State transitions (play/pause/end)
 * and seeks bypass this throttle so that scrubbing - which fires a
 * `pause` -> `seeked` -> `play` burst - is delivered intact.
 */
const HOST_HEARTBEAT_MS = 1000;
/** Position delta (s) above which an event is considered a seek. */
const SEEK_THRESHOLD_SECONDS = 0.75;

interface Args {
  /** Adapter mounted by the component, or null while not yet ready. */
  adapter: PlayerAdapter | null;
  /** Watch session being controlled. */
  session: WatchSession;
  /** Local user's session ID, or null when not yet known. */
  ownSession: number | null;
}

interface UseWatchSyncResult {
  /** True when local user is the authoritative host of this session. */
  isHost: boolean;
  /** True when remote drift exceeds the warning threshold. */
  outOfSync: boolean;
  /** Re-request authoritative state from the host. */
  requestState: () => Promise<void>;
  /** Send a `leave` event for the local user. */
  leave: () => Promise<void>;
  /** Send an `end` event (host only; UI should hide for non-hosts). */
  end: () => Promise<void>;
}

export function useWatchSync({ adapter, session, ownSession }: Args): UseWatchSyncResult {
  const { sendState, sendStateRequest, sendLeave, sendEnd } = useWatchSend();
  const lastHeartbeatRef = useRef(0);
  const lastSentRef = useRef<{ state: WatchPlaybackState; currentTime: number } | null>(null);
  const lastAppliedAtRef = useRef(0);
  const [outOfSync, setOutOfSync] = useState(false);

  const isHost = ownSession != null && ownSession === session.hostSession;

  // Forward local user events to the wire when host.
  useEffect(() => {
    if (!adapter || !isHost) return;
    const handler = (event: LocalPlayerEvent): void => {
      const now = Date.now();
      const last = lastSentRef.current;
      const stateChanged = last == null || last.state !== event.state;
      const seeked =
        last != null && Math.abs(last.currentTime - event.currentTime) > SEEK_THRESHOLD_SECONDS;
      const isImportant = stateChanged || seeked;
      // Throttle only steady-state heartbeats; transitions and seeks
      // must always go through so receivers can follow scrubs (which
      // arrive as a pause -> seeked -> play burst).
      if (!isImportant && now - lastHeartbeatRef.current < HOST_HEARTBEAT_MS) return;
      lastHeartbeatRef.current = now;
      lastSentRef.current = { state: event.state, currentTime: event.currentTime };
      void sendState(session.sessionId, {
        type: "state",
        state: event.state,
        currentTime: event.currentTime,
        updatedAtMs: now,
        hostSession: session.hostSession,
      });
    };
    // Adapters expose `setOnLocalEvent` so the same instance can be
    // re-bound across host/non-host transitions without a remount.
    adapter.setOnLocalEvent(handler);
    return () => adapter.setOnLocalEvent(undefined);
  }, [adapter, isHost, sendState, session.sessionId, session.hostSession]);

  // Apply remote authoritative state to the local adapter (non-hosts only).
  // Always follow the host - jumping in the video on the host's side is
  // an authoritative seek, not "drift".  The `outOfSync` flag is reserved
  // for the manual "Resync" button and is currently never raised
  // automatically because every state event causes a re-seek.
  useEffect(() => {
    if (!adapter || isHost) return;
    if (session.updatedAtMs === lastAppliedAtRef.current) return;
    lastAppliedAtRef.current = session.updatedAtMs;
    const expected = projectExpectedTime(session);
    const drift = Math.abs(adapter.currentTime() - expected);
    setOutOfSync(false);
    void applyRemoteState(adapter, session.state, expected, drift);
  }, [adapter, isHost, session]);

  const requestState = useCallback(
    () => sendStateRequest(session.sessionId),
    [sendStateRequest, session.sessionId],
  );
  const leave = useCallback(
    () => (ownSession == null ? Promise.resolve() : sendLeave(session.sessionId, ownSession)),
    [sendLeave, session.sessionId, ownSession],
  );
  const end = useCallback(() => sendEnd(session.sessionId), [sendEnd, session.sessionId]);

  return { isHost, outOfSync, requestState, leave, end };
}

/** Estimate the host's current playback position based on wall-clock drift. */
function projectExpectedTime(session: WatchSession): number {
  if (session.state !== "playing") return session.currentTime;
  const elapsed = (Date.now() - session.updatedAtMs) / 1000;
  return session.currentTime + Math.max(0, elapsed);
}

async function applyRemoteState(
  adapter: PlayerAdapter,
  state: WatchPlaybackState,
  at: number,
  drift: number,
): Promise<void> {
  const needsSeek = drift > DRIFT_RESYNC_SECONDS;
  switch (state) {
    case "playing":
      await adapter.play(at);
      break;
    case "paused":
      await adapter.pause(at);
      break;
    case "ended":
      await adapter.pause(at);
      break;
    default:
      if (needsSeek) await adapter.seek(at);
  }
}
