/**
 * Regression tests for the watch-together lifecycle effects:
 *
 * - participants are pruned when remote users disappear from the
 *   user list (fixes "x watching" never updating);
 * - hosts re-broadcast `start` + `state` when a brand-new user joins
 *   their channel (so newcomers can populate the local store
 *   without any chat history).
 *
 * The hook is exercised by mounting a tiny harness component so the
 * effects fire in a real React render cycle.
 */

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAppStore } from "../../store";
import type { UserEntry } from "../../types";
import { useWatchLifecycle } from "../chat/watch/useWatchLifecycle";
import { applyWatchSyncEvent } from "../chat/watch/watchStore";

const invokeMock = vi.fn().mockResolvedValue(undefined);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

function makeUser(session: number, channel: number, name = `u${session}`): UserEntry {
  return {
    session,
    name,
    channel_id: channel,
    texture: null,
    comment: null,
    self_mute: false,
    self_deaf: false,
    mute: false,
    deaf: false,
    suppress: false,
    talking: false,
    listening_channels: [],
    fancy_version: null,
    persistent_chat_capable: false,
    user_id: null,
    hash: null,
    custodian_state: null,
  } as unknown as UserEntry;
}

function reset(): void {
  useAppStore.setState({
    watchSessions: new Map(),
    watchSessionsVersion: 0,
    users: [],
    ownSession: null,
    currentChannel: null,
  });
  invokeMock.mockClear();
}

describe("useWatchLifecycle: participant pruning", () => {
  beforeEach(reset);
  afterEach(() => vi.clearAllMocks());

  it("removes a participant when they disappear from the user list", () => {
    useAppStore.setState({
      ownSession: 1,
      currentChannel: 9,
      users: [makeUser(1, 9), makeUser(2, 9), makeUser(3, 9)],
    });
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 1,
      event: { type: "start", channelId: 9, sourceUrl: "u", sourceKind: "directMedia" },
    });
    applyWatchSyncEvent({ sessionId: "s1", actor: 2, event: { type: "join", session: 2 } });
    applyWatchSyncEvent({ sessionId: "s1", actor: 3, event: { type: "join", session: 3 } });

    const { rerender } = renderHook(() => useWatchLifecycle());
    expect(useAppStore.getState().watchSessions.get("s1")?.participants.size).toBe(3);

    act(() => {
      useAppStore.setState({ users: [makeUser(1, 9), makeUser(3, 9)] });
    });
    rerender();

    const session = useAppStore.getState().watchSessions.get("s1");
    expect(session?.participants.has(2)).toBe(false);
    expect(session?.participants.size).toBe(2);
  });
});

describe("useWatchLifecycle: host re-advertise", () => {
  beforeEach(reset);
  afterEach(() => vi.clearAllMocks());

  it("re-broadcasts `start` and `state` when a new user appears in the host's channel", async () => {
    useAppStore.setState({
      ownSession: 1,
      currentChannel: 9,
      users: [makeUser(1, 9), makeUser(2, 9)],
    });
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 1,
      event: {
        type: "start",
        channelId: 9,
        sourceUrl: "https://example.com/v.mp4",
        sourceKind: "directMedia",
        title: "Movie",
      },
    });

    const { rerender } = renderHook(() => useWatchLifecycle());
    invokeMock.mockClear();

    await act(async () => {
      useAppStore.setState({ users: [makeUser(1, 9), makeUser(2, 9), makeUser(7, 9, "newcomer")] });
    });
    rerender();

    const sentEventTypes = invokeMock.mock.calls
      .filter(([cmd]) => cmd === "send_watch_sync")
      .map(([, args]) => (args as { event: { type: string } }).event.type);
    expect(sentEventTypes).toContain("start");
    expect(sentEventTypes).toContain("state");
  });

  it("does NOT re-broadcast when only existing users are present (no newcomers)", async () => {
    useAppStore.setState({
      ownSession: 1,
      currentChannel: 9,
      users: [makeUser(1, 9), makeUser(2, 9)],
    });
    applyWatchSyncEvent({
      sessionId: "s1",
      actor: 1,
      event: { type: "start", channelId: 9, sourceUrl: "u", sourceKind: "directMedia" },
    });

    const { rerender } = renderHook(() => useWatchLifecycle());
    invokeMock.mockClear();

    await act(async () => {
      // Identical user list, just a new array reference.
      useAppStore.setState({ users: [makeUser(1, 9), makeUser(2, 9)] });
    });
    rerender();

    const sendCalls = invokeMock.mock.calls.filter(([cmd]) => cmd === "send_watch_sync");
    expect(sendCalls.length).toBe(0);
  });
});
