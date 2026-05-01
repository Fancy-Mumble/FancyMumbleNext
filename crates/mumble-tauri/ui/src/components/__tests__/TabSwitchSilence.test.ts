/**
 * Regression test: switching server tabs must NOT trigger join/leave
 * notification sounds.
 *
 * Bug history: when `switchServer` runs it issues several sequential
 * `set(...)` calls on the Zustand store: first the new `users`/`channels`
 * (via `refreshState`), then the new `currentChannel`, and finally the
 * new `ownSession`.  In between those updates, `ownSession` is stale,
 * so the per-channel user set used by `useNotificationSounds` is built
 * filtering out the wrong session id.  When `ownSession` finally
 * arrives, the membership delta looks like a join (the new self is now
 * filtered out, the old self id - which may or may not be a real user
 * in the new channel - is no longer filtered) and the userJoinChannel
 * sound was emitted.
 *
 * This test reproduces that exact sequence and asserts the hook stays
 * silent.  This is the third recurrence of this bug, so the regression
 * net is intentionally explicit.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { UserEntry } from "../../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn().mockResolvedValue(true),
  requestPermission: vi.fn().mockResolvedValue("granted"),
  createChannel: vi.fn().mockResolvedValue(undefined),
  Importance: { Default: 3 },
  Visibility: { Public: 1 },
}));

vi.mock("@tauri-apps/plugin-store", () => ({
  load: vi.fn().mockResolvedValue({
    get: vi.fn().mockResolvedValue(null),
    set: vi.fn().mockResolvedValue(undefined),
  }),
}));

import { useAppStore } from "../../store";
import { useNotificationSounds } from "../../hooks/useNotificationSounds";
import { DEFAULT_NOTIFICATION_SOUNDS } from "../../pages/settings/NotificationsPanel";

// ----- Audio capture --------------------------------------------------

interface AudioCall {
  url: string;
  volume: number;
}

const audioCalls: AudioCall[] = [];

class FakeAudio {
  volume = 1;
  constructor(public url: string) {}
  play() {
    audioCalls.push({ url: this.url, volume: this.volume });
    return Promise.resolve();
  }
}

beforeEach(() => {
  audioCalls.length = 0;
  (globalThis as unknown as { Audio: typeof Audio }).Audio =
    FakeAudio as unknown as typeof Audio;
});

afterEach(() => {
  vi.restoreAllMocks();
});

function makeUser(session: number, channelId: number, name: string): UserEntry {
  return {
    session,
    name,
    channel_id: channelId,
    user_id: null,
    texture: null,
    texture_size: 0,
    comment: null,
    self_mute: false,
    self_deaf: false,
    mute: false,
    deaf: false,
    suppress: false,
    priority_speaker: false,
    recording: false,
    hash: null,
  } as unknown as UserEntry;
}

function fullyEnabledSettings() {
  return {
    ...DEFAULT_NOTIFICATION_SOUNDS,
    masterEnabled: true,
    events: {
      ...DEFAULT_NOTIFICATION_SOUNDS.events,
      userJoin: { ...DEFAULT_NOTIFICATION_SOUNDS.events.userJoin, enabled: true },
      userLeave: { ...DEFAULT_NOTIFICATION_SOUNDS.events.userLeave, enabled: true },
      userJoinChannel: {
        ...DEFAULT_NOTIFICATION_SOUNDS.events.userJoinChannel,
        enabled: true,
      },
      userLeaveChannel: {
        ...DEFAULT_NOTIFICATION_SOUNDS.events.userLeaveChannel,
        enabled: true,
      },
    },
  };
}

describe("useNotificationSounds - tab switching regression", () => {
  it("does not play join/leave sounds when switching server tabs", () => {
    // Step 0: set up server A as the active session with steady-state users
    // so the hook's snapshots are populated (no sound on first observation).
    act(() => {
      useAppStore.setState({
        activeServerId: "server-a",
        ownSession: 5,
        currentChannel: 7,
        users: [
          makeUser(5, 7, "me"),
          makeUser(11, 7, "alice"),
          makeUser(12, 8, "bob"),
        ],
      });
    });

    renderHook(() => useNotificationSounds(fullyEnabledSettings()));

    // First subscriber tick from a new state mutation populates the
    // snapshots silently.  Trigger a no-op update to flush.
    act(() => {
      useAppStore.setState({
        users: [
          makeUser(5, 7, "me"),
          makeUser(11, 7, "alice"),
          makeUser(12, 8, "bob"),
        ],
      });
    });

    audioCalls.length = 0; // ignore any setup noise

    // Step 1: switchServer first sets activeServerId.
    act(() => {
      useAppStore.setState({ activeServerId: "server-b" });
    });

    // Step 2: refreshState populates the new server's users/channels.
    // The new server has DIFFERENT users; crucially, the new
    // ownSession (99) is already present in the new users array at
    // this point, while the store's `ownSession` field is still 5.
    act(() => {
      useAppStore.setState({
        users: [
          makeUser(99, 3, "me-on-b"),
          makeUser(20, 3, "carol"),
          makeUser(21, 4, "dave"),
        ],
      });
    });

    // Step 3: switchServer sets currentChannel to the new server's channel.
    act(() => {
      useAppStore.setState({ currentChannel: 3 });
    });

    // Step 4: switchServer sets the real new ownSession last.
    act(() => {
      useAppStore.setState({ ownSession: 99 });
    });

    // Nothing in the above sequence represents a real user join or
    // leave - it's all just a tab switch.  Hook MUST remain silent.
    expect(audioCalls).toEqual([]);
  });

  it("still plays userJoinChannel for a real join after a tab switch", () => {
    act(() => {
      useAppStore.setState({
        activeServerId: "server-a",
        ownSession: 5,
        currentChannel: 7,
        users: [makeUser(5, 7, "me"), makeUser(11, 7, "alice")],
      });
    });

    renderHook(() => useNotificationSounds(fullyEnabledSettings()));

    // Flush initial snapshot.
    act(() => {
      useAppStore.setState({
        users: [makeUser(5, 7, "me"), makeUser(11, 7, "alice")],
      });
    });

    // Switch tabs (silent).
    act(() => {
      useAppStore.setState({ activeServerId: "server-b" });
    });
    act(() => {
      useAppStore.setState({
        users: [makeUser(99, 3, "me-on-b"), makeUser(20, 3, "carol")],
      });
    });
    act(() => {
      useAppStore.setState({ currentChannel: 3 });
    });
    act(() => {
      useAppStore.setState({ ownSession: 99 });
    });

    audioCalls.length = 0;

    // First post-switch user-set tick rebuilds the snapshot silently
    // (mirrors the existing activeServerId-reset behaviour).
    act(() => {
      useAppStore.setState({
        users: [
          makeUser(99, 3, "me-on-b"),
          makeUser(20, 3, "carol"),
          makeUser(30, 3, "eve"),
        ],
      });
    });

    // The next real join after the snapshot has settled MUST sound.
    act(() => {
      useAppStore.setState({
        users: [
          makeUser(99, 3, "me-on-b"),
          makeUser(20, 3, "carol"),
          makeUser(30, 3, "eve"),
          makeUser(31, 3, "frank"),
        ],
      });
    });

    expect(audioCalls.length).toBeGreaterThan(0);
  });
});
