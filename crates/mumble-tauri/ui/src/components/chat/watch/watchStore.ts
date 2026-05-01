/**
 * Reducer for incoming `watch-sync` Tauri events.
 *
 * Translates a single `WatchSyncPayload` into mutations on the
 * `watchSessions` map of the Zustand store.  Kept in a separate file
 * so the store stays focused and the logic is unit-testable in
 * isolation (see `__tests__/WatchStoreProcessing.test.ts`).
 */

import { useAppStore } from "../../../store";
import type {
  WatchPlaybackState,
  WatchSession,
  WatchSourceKind,
  WatchSyncPayload,
} from "./watchTypes";

/** Drops payloads we cannot act on and returns true if processing should continue. */
function isActionable(payload: WatchSyncPayload): payload is Required<Pick<WatchSyncPayload, "sessionId" | "actor">> & WatchSyncPayload {
  return payload.sessionId != null && payload.actor != null;
}

export function applyWatchSyncEvent(payload: WatchSyncPayload): void {
  if (!isActionable(payload)) return;
  const { sessionId, actor, event } = payload;

  useAppStore.setState((prev) => {
    const sessions = new Map(prev.watchSessions);
    const existing = sessions.get(sessionId);

    switch (event.type) {
      case "start":
        applyStart(sessions, sessionId, actor, event);
        break;
      case "state":
        if (existing) applyState(sessions, sessionId, existing, event);
        break;
      case "join":
        if (existing) applyJoin(sessions, sessionId, existing, event.session ?? actor);
        break;
      case "leave":
        if (existing) applyLeave(sessions, sessionId, existing, event.session ?? actor);
        break;
      case "stateRequest":
        // Host responds via `state`; non-hosts ignore.
        return prev;
      case "end":
        sessions.delete(sessionId);
        break;
      case "hostTransfer":
        if (existing && event.newHostSession != null) {
          sessions.set(sessionId, { ...existing, hostSession: event.newHostSession });
        }
        break;
    }

    if (sessions === prev.watchSessions) return prev;
    return {
      watchSessions: sessions,
      watchSessionsVersion: prev.watchSessionsVersion + 1,
    };
  });
}

function applyStart(
  sessions: Map<string, WatchSession>,
  sessionId: string,
  actor: number,
  event: Extract<WatchSyncPayload["event"], { type: "start" }>,
): void {
  if (event.sourceUrl == null || event.channelId == null) return;
  const hostSession = event.hostSession ?? actor;
  const existing = sessions.get(sessionId);
  const session: WatchSession = {
    sessionId,
    channelId: event.channelId,
    hostSession,
    sourceUrl: event.sourceUrl,
    sourceKind: (event.sourceKind ?? "directMedia") as WatchSourceKind,
    title: event.title,
    participants: existing?.participants ?? new Set([hostSession]),
    state: existing?.state ?? "paused",
    currentTime: existing?.currentTime ?? 0,
    updatedAtMs: existing?.updatedAtMs ?? Date.now(),
  };
  sessions.set(sessionId, session);
}

function applyState(
  sessions: Map<string, WatchSession>,
  sessionId: string,
  existing: WatchSession,
  event: Extract<WatchSyncPayload["event"], { type: "state" }>,
): void {
  sessions.set(sessionId, {
    ...existing,
    state: (event.state ?? existing.state) as WatchPlaybackState,
    currentTime: event.currentTime ?? existing.currentTime,
    updatedAtMs: event.updatedAtMs ?? existing.updatedAtMs,
    hostSession: event.hostSession ?? existing.hostSession,
  });
}

function applyJoin(
  sessions: Map<string, WatchSession>,
  sessionId: string,
  existing: WatchSession,
  session: number,
): void {
  if (existing.participants.has(session)) return;
  const participants = new Set(existing.participants);
  participants.add(session);
  sessions.set(sessionId, { ...existing, participants });
}

function applyLeave(
  sessions: Map<string, WatchSession>,
  sessionId: string,
  existing: WatchSession,
  session: number,
): void {
  if (!existing.participants.has(session)) return;
  const participants = new Set(existing.participants);
  participants.delete(session);
  sessions.set(sessionId, { ...existing, participants });
}
