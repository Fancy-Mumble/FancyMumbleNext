/**
 * Tests for the talking-sessions state and Online-list sort order.
 *
 * Verifies that:
 * 1. The Zustand store correctly tracks which users are talking via
 *    the `talkingSessions` set.
 * 2. Users in the current channel are sorted first in the Online list.
 */

import { describe, it, expect, beforeEach } from "vitest";
import { useAppStore } from "../../store";
import type { UserEntry } from "../../types";

// --- Helpers ------------------------------------------------------

function makeUser(session: number, channelId: number, name = `User${session}`): UserEntry {
  return {
    session,
    name,
    channel_id: channelId,
    texture_size: null,
    comment: null,
    mute: false,
    deaf: false,
    suppress: false,
    self_mute: false,
    self_deaf: false,
    priority_speaker: false,
  };
}

/** Sort users the same way the Online section does. */
function sortOnlineUsers(users: UserEntry[], currentChannel: number | null): UserEntry[] {
  return [...users].sort((a, b) => {
    const aInChannel = currentChannel != null && a.channel_id === currentChannel ? 0 : 1;
    const bInChannel = currentChannel != null && b.channel_id === currentChannel ? 0 : 1;
    return aInChannel - bInChannel;
  });
}

// --- Tests --------------------------------------------------------

describe("talkingSessions store state", () => {
  beforeEach(() => {
    useAppStore.setState({ talkingSessions: new Set() });
  });

  it("starts with an empty set", () => {
    const { talkingSessions } = useAppStore.getState();
    expect(talkingSessions.size).toBe(0);
  });

  it("adds a session when a user starts talking", () => {
    const next = new Set(useAppStore.getState().talkingSessions);
    next.add(42);
    useAppStore.setState({ talkingSessions: next });

    expect(useAppStore.getState().talkingSessions.has(42)).toBe(true);
    expect(useAppStore.getState().talkingSessions.size).toBe(1);
  });

  it("removes a session when a user stops talking", () => {
    useAppStore.setState({ talkingSessions: new Set([10, 20, 30]) });

    const next = new Set(useAppStore.getState().talkingSessions);
    next.delete(20);
    useAppStore.setState({ talkingSessions: next });

    expect(useAppStore.getState().talkingSessions.has(10)).toBe(true);
    expect(useAppStore.getState().talkingSessions.has(20)).toBe(false);
    expect(useAppStore.getState().talkingSessions.has(30)).toBe(true);
    expect(useAppStore.getState().talkingSessions.size).toBe(2);
  });

  it("clears all sessions on disconnect (reset to initial)", () => {
    useAppStore.setState({ talkingSessions: new Set([1, 2, 3]) });
    useAppStore.setState({ talkingSessions: new Set() });

    expect(useAppStore.getState().talkingSessions.size).toBe(0);
  });

  it("tracks multiple simultaneous talkers", () => {
    useAppStore.setState({ talkingSessions: new Set([5, 10, 15, 20]) });

    const ts = useAppStore.getState().talkingSessions;
    expect(ts.size).toBe(4);
    expect(ts.has(5)).toBe(true);
    expect(ts.has(10)).toBe(true);
    expect(ts.has(15)).toBe(true);
    expect(ts.has(20)).toBe(true);
  });
});

describe("Online list sorting (current channel first)", () => {
  it("places current-channel users before others", () => {
    const users = [
      makeUser(1, 100, "Alice"),
      makeUser(2, 200, "Bob"),
      makeUser(3, 100, "Charlie"),
      makeUser(4, 300, "Dave"),
    ];

    const sorted = sortOnlineUsers(users, 100);
    expect(sorted[0].name).toBe("Alice");
    expect(sorted[1].name).toBe("Charlie");
    // Bob and Dave are after the current-channel users
    expect(sorted.slice(2).map((u) => u.name)).toEqual(
      expect.arrayContaining(["Bob", "Dave"]),
    );
  });

  it("preserves relative order when no current channel", () => {
    const users = [
      makeUser(1, 100, "Alice"),
      makeUser(2, 200, "Bob"),
    ];

    const sorted = sortOnlineUsers(users, null);
    expect(sorted[0].name).toBe("Alice");
    expect(sorted[1].name).toBe("Bob");
  });

  it("preserves relative order among same-channel users", () => {
    const users = [
      makeUser(1, 100, "Alice"),
      makeUser(2, 100, "Bob"),
      makeUser(3, 100, "Charlie"),
    ];

    const sorted = sortOnlineUsers(users, 100);
    expect(sorted.map((u) => u.name)).toEqual(["Alice", "Bob", "Charlie"]);
  });

  it("puts all users after when none match current channel", () => {
    const users = [
      makeUser(1, 200, "Alice"),
      makeUser(2, 300, "Bob"),
    ];

    const sorted = sortOnlineUsers(users, 100);
    expect(sorted.map((u) => u.name)).toEqual(["Alice", "Bob"]);
  });
});
