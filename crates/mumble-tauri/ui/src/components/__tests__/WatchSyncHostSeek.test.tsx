/**
 * Regression test for the seek-not-propagating bug.
 *
 * When the host scrubs the HTML5 <video>, the browser emits a
 * `pause` -> `seeked` -> `play` burst within ~50 ms.  The previous
 * 1 Hz throttle dropped the second and third events, so receivers
 * saw "paused at old position" forever.  The fixed throttle bypasses
 * the rate limit for any state transition or any meaningful
 * currentTime change (a seek), so all three events go out and
 * receivers end up at the new position, playing.
 */

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAppStore } from "../../store";
import { useWatchSync } from "../chat/watch/useWatchSync";
import type { LocalPlayerEvent, PlayerAdapter } from "../chat/watch/PlayerAdapter";
import type { WatchSession } from "../chat/watch/watchTypes";

const invokeMock = vi.fn().mockResolvedValue(undefined);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

function makeAdapter(): {
  adapter: PlayerAdapter;
  emit: (event: LocalPlayerEvent) => void;
} {
  let cb: ((e: LocalPlayerEvent) => void) | undefined;
  const adapter: PlayerAdapter = {
    play: vi.fn().mockResolvedValue(undefined),
    pause: vi.fn().mockResolvedValue(undefined),
    seek: vi.fn().mockResolvedValue(undefined),
    currentTime: () => 0,
    setOnLocalEvent: (next) => {
      cb = next;
    },
    destroy: () => undefined,
  };
  return { adapter, emit: (e) => cb?.(e) };
}

function makeSession(): WatchSession {
  return {
    sessionId: "s1",
    channelId: 9,
    hostSession: 1,
    sourceUrl: "https://example.com/v.mp4",
    sourceKind: "directMedia",
    title: "Movie",
    participants: new Set([1]),
    state: "paused",
    currentTime: 0,
    updatedAtMs: 0,
  };
}

function reset(): void {
  useAppStore.setState({
    watchSessions: new Map(),
    watchSessionsVersion: 0,
    users: [],
    ownSession: 1,
  });
  invokeMock.mockClear();
}

function watchSyncCalls(): { event: { type: string; state?: string; currentTime?: number } }[] {
  return invokeMock.mock.calls
    .filter(([cmd]) => cmd === "send_watch_sync")
    .map(([, args]) => args as { event: { type: string; state?: string; currentTime?: number } });
}

describe("useWatchSync host throttle", () => {
  beforeEach(reset);
  afterEach(() => vi.clearAllMocks());

  it("forwards a pause -> seeked -> play burst without dropping events", async () => {
    const { adapter, emit } = makeAdapter();
    const session = makeSession();

    renderHook(() => useWatchSync({ adapter, session, ownSession: 1 }));

    await act(async () => {
      // Host was playing at 10s; user grabs the scrub bar.
      emit({ state: "paused", currentTime: 10 });
      // Browser fires `seeked` with paused state at the new position.
      emit({ state: "paused", currentTime: 60 });
      // Then `play` resumes.
      emit({ state: "playing", currentTime: 60 });
    });

    const events = watchSyncCalls().map((c) => c.event);
    expect(events).toHaveLength(3);
    expect(events[0]).toMatchObject({ type: "state", state: "paused", currentTime: 10 });
    expect(events[1]).toMatchObject({ type: "state", state: "paused", currentTime: 60 });
    expect(events[2]).toMatchObject({ type: "state", state: "playing", currentTime: 60 });
  });

  it("still throttles repeated steady-state heartbeats", async () => {
    const { adapter, emit } = makeAdapter();
    const session = makeSession();

    renderHook(() => useWatchSync({ adapter, session, ownSession: 1 }));

    await act(async () => {
      emit({ state: "playing", currentTime: 10 });
      // Same state, tiny advance: should be dropped.
      emit({ state: "playing", currentTime: 10.1 });
      emit({ state: "playing", currentTime: 10.2 });
    });

    expect(watchSyncCalls()).toHaveLength(1);
  });

  it("forwards a same-state seek (scrubbing while paused) immediately", async () => {
    const { adapter, emit } = makeAdapter();
    const session = makeSession();

    renderHook(() => useWatchSync({ adapter, session, ownSession: 1 }));

    await act(async () => {
      emit({ state: "paused", currentTime: 10 });
      // Scrub while paused: same state, large position jump.
      emit({ state: "paused", currentTime: 90 });
    });

    const events = watchSyncCalls().map((c) => c.event);
    expect(events).toHaveLength(2);
    expect(events[1]).toMatchObject({ state: "paused", currentTime: 90 });
  });
});

describe("useWatchSync non-host follow", () => {
  beforeEach(reset);
  afterEach(() => vi.clearAllMocks());

  it("seeks to the host's new position on a large jump instead of going out-of-sync", async () => {
    const { adapter } = makeAdapter();
    // Non-host: ownSession (2) is not session.hostSession (1).
    let session: WatchSession = {
      ...makeSession(),
      state: "playing",
      currentTime: 10,
      updatedAtMs: 1000,
    };
    (adapter.currentTime as unknown as () => number) = () => 10;

    const { result, rerender } = renderHook(
      ({ s }: { s: WatchSession }) => useWatchSync({ adapter, session: s, ownSession: 2 }),
      { initialProps: { s: session } },
    );

    // Host seeks from 10s to 90s.
    session = { ...session, currentTime: 90, updatedAtMs: 2000 };
    await act(async () => {
      rerender({ s: session });
    });

    expect(adapter.play).toHaveBeenCalled();
    const playArg = (adapter.play as unknown as { mock: { calls: number[][] } }).mock.calls[0][0];
    expect(playArg).toBeGreaterThanOrEqual(90);
    expect(result.current.outOfSync).toBe(false);
  });
});
