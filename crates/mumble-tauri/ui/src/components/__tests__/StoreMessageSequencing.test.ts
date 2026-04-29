/**
 * Regression tests for message write sequencing in the Zustand store.
 *
 * Verifies that concurrent async operations (selectChannel,
 * refreshMessages, sendMessage) do not overwrite each other's results
 * with stale data.  This was the root cause of persisted messages
 * "disappearing" when a pchat fetch response arrived while
 * selectChannel was still awaiting its own get_messages invoke.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ChatMessage } from "../../types";

// -- Controllable invoke mock --------------------------------------
// We queue deferred promises so we can resolve them in arbitrary order.

type Deferred = { cmd: string; args: unknown; resolve: (v: unknown) => void; reject: (e: unknown) => void };
const deferred: Deferred[] = [];

function createInvokeMock() {
  return vi.fn((cmd: string, args?: unknown) =>
    new Promise((resolve, reject) => {
      deferred.push({ cmd, args, resolve, reject });
    }),
  );
}

const invokeMock = createInvokeMock();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(args[0] as string, args[1]),
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

// Import after mocks.
import { useAppStore } from "../../store";

// -- Helpers -------------------------------------------------------

function makeMsg(id: string, body: string, channelId = 1): ChatMessage {
  return {
    sender_session: 10,
    sender_name: "User",
    body,
    channel_id: channelId,
    is_own: false,
    dm_session: null,
    message_id: id,
    timestamp: Date.now(),
    is_legacy: false,
  };
}

/** Resolve the first pending invoke matching cmd and remove it. */
function resolveNext(cmd: string, value: unknown): void {
  const idx = deferred.findIndex((d) => d.cmd === cmd);
  if (idx === -1) throw new Error(`no pending invoke for "${cmd}"`);
  const [entry] = deferred.splice(idx, 1);
  entry.resolve(value);
}

/** Drain the microtask queue. */
async function tick(): Promise<void> {
  // Three rounds is enough for multi-level async chains.
  for (let i = 0; i < 3; i++) {
    await Promise.resolve();
  }
}

// -- Setup ---------------------------------------------------------

beforeEach(() => {
  deferred.length = 0;
  invokeMock.mockClear();
  useAppStore.setState({
    selectedChannel: null,
    messages: [],
    status: "connected",
  });
});

// -- Tests ---------------------------------------------------------

describe("message write sequencing (regression)", () => {
  it("stale selectChannel does not overwrite fresher refreshMessages", async () => {
    const stale = [makeMsg("m1", "old")];
    const fresh = [makeMsg("m1", "old"), makeMsg("m2", "new")];

    // 1. Fire selectChannel — it immediately sets selectedChannel, then
    //    awaits invoke("select_channel").
    const p1 = useAppStore.getState().selectChannel(1);
    // p1 is floating — we won't await it until the end.

    // select_channel should be pending.
    expect(deferred.some((d) => d.cmd === "select_channel")).toBe(true);

    // 2. Resolve select_channel so selectChannel proceeds to get_messages.
    resolveNext("select_channel", undefined);
    await tick();

    // get_messages (for selectChannel) should now be pending.
    expect(deferred.filter((d) => d.cmd === "get_messages")).toHaveLength(1);

    // 3. While selectChannel's get_messages is still pending, fire
    //    refreshMessages (simulating a new-message event handler).
    const p2 = useAppStore.getState().refreshMessages(1);

    // Two get_messages calls pending.
    expect(deferred.filter((d) => d.cmd === "get_messages")).toHaveLength(2);

    // 4. Resolve refreshMessages' get_messages (second one) FIRST.
    //    deferred[1] is the second get_messages (from refreshMessages).
    deferred[1].resolve(fresh);
    deferred.splice(1, 1);
    await tick();
    // p2 should now be resolved.
    await p2;

    expect(useAppStore.getState().messages).toHaveLength(2);

    // 5. Resolve selectChannel's get_messages (first one) with stale data.
    resolveNext("get_messages", stale);
    await tick();
    await p1;

    // Stale write must have been discarded.
    expect(useAppStore.getState().messages).toHaveLength(2);
    expect(useAppStore.getState().messages.map((m) => m.message_id)).toEqual(["m1", "m2"]);
  });

  it("rapid channel switching only keeps the last channel's messages", async () => {
    const ch1 = [makeMsg("c1", "channel one", 1)];
    const ch2 = [makeMsg("c2", "channel two", 2)];

    // Select channel 1.
    const p1 = useAppStore.getState().selectChannel(1);
    resolveNext("select_channel", undefined);
    await tick();

    // Before ch1's get_messages resolves, switch to channel 2.
    const p2 = useAppStore.getState().selectChannel(2);
    resolveNext("select_channel", undefined);
    await tick();

    // Resolve ch2's get_messages first.
    // deferred now has two get_messages: [0]=ch1, [1]=ch2.
    deferred[1].resolve(ch2);
    deferred.splice(1, 1);
    await tick();
    await p2;

    expect(useAppStore.getState().selectedChannel).toBe(2);
    expect(useAppStore.getState().messages).toEqual(ch2);

    // Resolve ch1's stale get_messages.
    resolveNext("get_messages", ch1);
    await tick();
    await p1;

    // Channel 1's data must not replace channel 2's.
    expect(useAppStore.getState().messages).toEqual(ch2);
  });
});
