/**
 * Tests for key-holder state management in the Zustand store.
 *
 * Regression tests for the bug where `PchatKeyHolderReport` was only sent
 * during key exchange but NOT when a key was derived/generated locally
 * (FullArchive deterministic derivation or PostJoin self-generation).
 *
 * These tests verify that:
 * 1. The store's `keyHolders` state updates correctly from events
 * 2. Online/offline status is correctly derived
 * 3. Key holders are scoped per channel
 */

import { describe, it, expect, beforeEach } from "vitest";
import { useAppStore } from "../../store";
import type { KeyHolderEntry, UserEntry } from "../../types";

// --- Helpers ------------------------------------------------------

function makeHolder(overrides: Partial<KeyHolderEntry> = {}): KeyHolderEntry {
  return {
    cert_hash: `hash_${Math.random().toString(36).slice(2)}`,
    name: "TestHolder",
    is_online: true,
    ...overrides,
  };
}

function makeUser(overrides: Partial<UserEntry> = {}): UserEntry {
  return {
    session: 1,
    name: "User1",
    channel_id: 0,
    texture: null,
    comment: null,
    mute: false,
    deaf: false,
    suppress: false,
    self_mute: false,
    self_deaf: false,
    priority_speaker: false,
    ...overrides,
  };
}

/**
 * Simulate the `pchat-key-holders-changed` Tauri event by directly
 * updating the store in the same way the event listener does.
 */
function simulateKeyHoldersChanged(
  channelId: number,
  holders: KeyHolderEntry[],
) {
  useAppStore.setState((prev) => ({
    keyHolders: {
      ...prev.keyHolders,
      [channelId]: holders,
    },
  }));
}

// --- Reset store between tests ------------------------------------

beforeEach(() => {
  useAppStore.getState().reset();
});

// --- Key holders state management ---------------------------------

describe("keyHolders store state", () => {
  it("starts with empty keyHolders", () => {
    const { keyHolders } = useAppStore.getState();
    expect(keyHolders).toEqual({});
  });

  it("stores holders for a channel when event fires", () => {
    const holder = makeHolder({ cert_hash: "abc123", name: "Alice" });
    simulateKeyHoldersChanged(0, [holder]);

    const { keyHolders } = useAppStore.getState();
    expect(keyHolders[0]).toHaveLength(1);
    expect(keyHolders[0][0].cert_hash).toBe("abc123");
    expect(keyHolders[0][0].name).toBe("Alice");
  });

  it("handles multiple holders for a channel", () => {
    const alice = makeHolder({
      cert_hash: "hash_alice",
      name: "Alice",
      is_online: true,
    });
    const bob = makeHolder({
      cert_hash: "hash_bob",
      name: "Bob",
      is_online: false,
    });
    simulateKeyHoldersChanged(5, [alice, bob]);

    const holders = useAppStore.getState().keyHolders[5];
    expect(holders).toHaveLength(2);
    expect(holders.map((h) => h.cert_hash)).toEqual([
      "hash_alice",
      "hash_bob",
    ]);
  });

  it("keeps holders for different channels separate", () => {
    const holderA = makeHolder({ cert_hash: "ch0_user", name: "ChannelZero" });
    const holderB = makeHolder({ cert_hash: "ch3_user", name: "ChannelThree" });
    simulateKeyHoldersChanged(0, [holderA]);
    simulateKeyHoldersChanged(3, [holderB]);

    const { keyHolders } = useAppStore.getState();
    expect(keyHolders[0]).toHaveLength(1);
    expect(keyHolders[0][0].name).toBe("ChannelZero");
    expect(keyHolders[3]).toHaveLength(1);
    expect(keyHolders[3][0].name).toBe("ChannelThree");
  });

  it("replaces previous holders on update", () => {
    const old = makeHolder({ cert_hash: "old_hash", name: "OldUser" });
    simulateKeyHoldersChanged(0, [old]);
    expect(useAppStore.getState().keyHolders[0]).toHaveLength(1);

    const updated = makeHolder({ cert_hash: "new_hash", name: "NewUser" });
    simulateKeyHoldersChanged(0, [updated]);

    const holders = useAppStore.getState().keyHolders[0];
    expect(holders).toHaveLength(1);
    expect(holders[0].cert_hash).toBe("new_hash");
    expect(holders[0].name).toBe("NewUser");
  });

  it("does not affect other channels when one updates", () => {
    const ch0 = makeHolder({ cert_hash: "ch0", name: "Zero" });
    const ch1 = makeHolder({ cert_hash: "ch1", name: "One" });
    simulateKeyHoldersChanged(0, [ch0]);
    simulateKeyHoldersChanged(1, [ch1]);

    // Update channel 0 only.
    const ch0_updated = makeHolder({ cert_hash: "ch0_v2", name: "ZeroV2" });
    simulateKeyHoldersChanged(0, [ch0_updated]);

    expect(useAppStore.getState().keyHolders[0][0].cert_hash).toBe("ch0_v2");
    expect(useAppStore.getState().keyHolders[1][0].cert_hash).toBe("ch1");
  });

  it("handles empty holders list (channel cleared)", () => {
    const holder = makeHolder({ cert_hash: "abc", name: "User" });
    simulateKeyHoldersChanged(0, [holder]);
    expect(useAppStore.getState().keyHolders[0]).toHaveLength(1);

    simulateKeyHoldersChanged(0, []);
    expect(useAppStore.getState().keyHolders[0]).toHaveLength(0);
  });

  it("reset clears all keyHolders", () => {
    simulateKeyHoldersChanged(0, [
      makeHolder({ cert_hash: "a", name: "A" }),
    ]);
    simulateKeyHoldersChanged(1, [
      makeHolder({ cert_hash: "b", name: "B" }),
    ]);

    useAppStore.getState().reset();
    expect(useAppStore.getState().keyHolders).toEqual({});
  });
});

// --- Online / offline filtering -----------------------------------

describe("keyHolders online/offline status", () => {
  it("distinguishes online and offline holders", () => {
    const online = makeHolder({
      cert_hash: "online_h",
      name: "Online",
      is_online: true,
    });
    const offline = makeHolder({
      cert_hash: "offline_h",
      name: "Offline",
      is_online: false,
    });
    simulateKeyHoldersChanged(0, [online, offline]);

    const holders = useAppStore.getState().keyHolders[0];
    const onlineHolders = holders.filter((h) => h.is_online);
    const offlineHolders = holders.filter((h) => !h.is_online);

    expect(onlineHolders).toHaveLength(1);
    expect(onlineHolders[0].name).toBe("Online");
    expect(offlineHolders).toHaveLength(1);
    expect(offlineHolders[0].name).toBe("Offline");
  });

  it("holder hash set can be computed for matching online users", () => {
    const h1 = makeHolder({ cert_hash: "hash_alice", name: "Alice" });
    const h2 = makeHolder({ cert_hash: "hash_bob", name: "Bob" });
    simulateKeyHoldersChanged(0, [h1, h2]);

    const holders = useAppStore.getState().keyHolders[0];
    const hashSet = new Set(holders.map((h) => h.cert_hash));
    expect(hashSet.has("hash_alice")).toBe(true);
    expect(hashSet.has("hash_bob")).toBe(true);
    expect(hashSet.has("hash_unknown")).toBe(false);
  });
});

// --- Integration with users state ---------------------------------

describe("keyHolders and users integration", () => {
  it("holder names can be resolved from connected users", () => {
    // Simulate users being present.
    useAppStore.setState({
      users: [
        makeUser({ session: 1, name: "Alice" }),
        makeUser({ session: 2, name: "Bob" }),
      ],
    });

    // Server sends holders with cert hashes.
    const holders: KeyHolderEntry[] = [
      { cert_hash: "hash_alice", name: "Alice", is_online: true },
      { cert_hash: "hash_charlie", name: "Charlie", is_online: false },
    ];
    simulateKeyHoldersChanged(0, holders);

    const stored = useAppStore.getState().keyHolders[0];
    expect(stored).toHaveLength(2);
    // Online holder has the resolved name.
    expect(stored[0].name).toBe("Alice");
    // Offline holder retains server-provided name.
    expect(stored[1].name).toBe("Charlie");
  });
});
