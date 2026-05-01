/**
 * Regression test for the bug where the watch-together requester
 * never saw their own session locally because the server does not
 * echo `FancyWatchSync` back to the sender. `useWatchStart.start()`
 * must apply the `start` event to the local store before sending it
 * to the wire so the card can render immediately.
 */

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAppStore } from "../../store";
import { useWatchStart } from "../chat/watch/useWatchStart";
import { _resetPendingAutoStartForTests, consumePendingAutoStart } from "../chat/watch/watchAutoStart";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

function reset(): void {
  useAppStore.setState({
    watchSessions: new Map(),
    watchSessionsVersion: 0,
    ownSession: 42,
  });
  _resetPendingAutoStartForTests();
}

describe("useWatchStart.start", () => {
  beforeEach(reset);
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("creates a local watch session synchronously, before the server echo", async () => {
    const sendMessageSpy = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({ sendMessage: sendMessageSpy });

    const body = "Check this out https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    const { result } = renderHook(() => useWatchStart(body, 9));

    expect(result.current.canStart).toBe(true);
    expect(useAppStore.getState().watchSessions.size).toBe(0);

    await act(async () => {
      await result.current.start();
    });

    const sessions = useAppStore.getState().watchSessions;
    expect(sessions.size).toBe(1);
    const session = Array.from(sessions.values())[0];
    expect(session.hostSession).toBe(42);
    expect(session.channelId).toBe(9);
    expect(session.participants.has(42)).toBe(true);
    expect(session.sourceKind).toBe("youtube");
    expect(sendMessageSpy).toHaveBeenCalledTimes(1);
    expect(sendMessageSpy.mock.calls[0]?.[1]).toMatch(/<!-- FANCY_WATCH:[0-9a-f-]+ -->/);
  });

  it("marks the new session as pending auto-start so the host plays on mount", async () => {
    const sendMessageSpy = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({ sendMessage: sendMessageSpy });

    const body = "Movie https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    const { result } = renderHook(() => useWatchStart(body, 9));

    await act(async () => {
      await result.current.start();
    });

    const session = Array.from(useAppStore.getState().watchSessions.values())[0];
    expect(consumePendingAutoStart(session.sessionId)).toBe(true);
    // Subsequent consumption returns false (one-shot).
    expect(consumePendingAutoStart(session.sessionId)).toBe(false);
  });
});
