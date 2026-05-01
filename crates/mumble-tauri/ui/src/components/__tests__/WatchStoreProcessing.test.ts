/**
 * Unit tests for `applyWatchSyncEvent` (the watch-sync event reducer
 * for the Zustand store) and `electHost` (deterministic host
 * election used during lifecycle re-election).
 */

import { describe, it, expect, beforeEach } from "vitest";
import { useAppStore } from "../../store";
import { applyWatchSyncEvent } from "../chat/watch/watchStore";
import { electHost } from "../chat/watch/useWatchLifecycle";

function reset(): void {
  useAppStore.setState({
    watchSessions: new Map(),
    watchSessionsVersion: 0,
  });
}

describe("applyWatchSyncEvent", () => {
  beforeEach(reset);

  it("ignores payloads without sessionId or actor", () => {
    applyWatchSyncEvent({ event: { type: "stateRequest" } });
    expect(useAppStore.getState().watchSessions.size).toBe(0);
  });

  it("creates a session on `start`", () => {
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 7,
      event: {
        type: "start",
        channelId: 5,
        sourceUrl: "https://example.com/x.mp4",
        sourceKind: "directMedia",
        title: "Movie",
      },
    });
    const s = useAppStore.getState().watchSessions.get("s1");
    expect(s).toBeDefined();
    expect(s?.channelId).toBe(5);
    expect(s?.hostSession).toBe(7);
    expect(s?.title).toBe("Movie");
    expect(s?.participants.has(7)).toBe(true);
    expect(useAppStore.getState().watchSessionsVersion).toBe(1);
  });

  it("ignores `start` without sourceUrl/channelId", () => {
    applyWatchSyncEvent({ sessionId: "s1", actor: 7, event: { type: "start" } });
    expect(useAppStore.getState().watchSessions.size).toBe(0);
  });

  it("updates state via `state`", () => {
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 7,
      event: {
        type: "start",
        channelId: 5,
        sourceUrl: "https://example.com/x.mp4",
        sourceKind: "directMedia",
      },
    });
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 7,
      event: { type: "state", state: "playing", currentTime: 12.5, updatedAtMs: 100 },
    });
    const s = useAppStore.getState().watchSessions.get("s1")!;
    expect(s.state).toBe("playing");
    expect(s.currentTime).toBe(12.5);
    expect(s.updatedAtMs).toBe(100);
  });

  it("adds and removes participants on join/leave", () => {
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 1,
      event: { type: "start", channelId: 5, sourceUrl: "x", sourceKind: "directMedia" },
    });
    applyWatchSyncEvent({ sessionId: "s1", actor: 2, event: { type: "join", session: 2 } });
    applyWatchSyncEvent({ sessionId: "s1", actor: 3, event: { type: "join", session: 3 } });
    expect(useAppStore.getState().watchSessions.get("s1")?.participants.size).toBe(3);

    applyWatchSyncEvent({ sessionId: "s1", actor: 2, event: { type: "leave", session: 2 } });
    expect(useAppStore.getState().watchSessions.get("s1")?.participants.has(2)).toBe(false);
  });

  it("transfers host on hostTransfer", () => {
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 1,
      event: { type: "start", channelId: 5, sourceUrl: "x", sourceKind: "directMedia" },
    });
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 9,
      event: { type: "hostTransfer", newHostSession: 9 },
    });
    expect(useAppStore.getState().watchSessions.get("s1")?.hostSession).toBe(9);
  });

  it("deletes the session on `end`", () => {
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 1,
      event: { type: "start", channelId: 5, sourceUrl: "x", sourceKind: "directMedia" },
    });
    applyWatchSyncEvent({ sessionId: "s1", actor: 1, event: { type: "end" } });
    expect(useAppStore.getState().watchSessions.has("s1")).toBe(false);
  });

  it("preserves participants when `start` is replayed", () => {
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 1,
      event: { type: "start", channelId: 5, sourceUrl: "x", sourceKind: "directMedia" },
    });
    applyWatchSyncEvent({ sessionId: "s1", actor: 4, event: { type: "join", session: 4 } });
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 1,
      event: { type: "start", channelId: 5, sourceUrl: "x", sourceKind: "directMedia" },
    });
    expect(useAppStore.getState().watchSessions.get("s1")?.participants.has(4)).toBe(true);
  });
});

describe("electHost", () => {
  it("returns null for an empty candidate set", () => {
    expect(electHost([], "s1")).toBeNull();
  });

  it("is deterministic across calls", () => {
    const a = electHost([3, 5, 11, 42], "session-uuid");
    const b = electHost([42, 11, 3, 5], "session-uuid");
    expect(a).toBe(b);
  });

  it("differs across session UUIDs (avoids permanent host bias)", () => {
    const candidates = [3, 5, 11, 42];
    const winners = new Set([
      electHost(candidates, "abc"),
      electHost(candidates, "def"),
      electHost(candidates, "ghi"),
      electHost(candidates, "jkl"),
      electHost(candidates, "mno"),
    ]);
    expect(winners.size).toBeGreaterThan(1);
  });

  it("only picks a session present in the candidate set", () => {
    const winner = electHost([3, 5, 11, 42], "anything");
    expect([3, 5, 11, 42]).toContain(winner);
  });
});
