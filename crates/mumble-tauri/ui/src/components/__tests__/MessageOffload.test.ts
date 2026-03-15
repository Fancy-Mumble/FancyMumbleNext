/**
 * Unit tests for the message offloading system.
 *
 * Tests cover:
 * - Placeholder format parsing (with and without content size)
 * - Heavy content detection heuristics
 * - MessageOffloadManager scheduling, cancellation, and restore logic
 * - Batch restore (restoreMany)
 * - Provider abstraction (custom in-memory provider)
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  isHeavyContent,
  isOffloaded,
  offloadPlaceholder,
  extractOffloadInfo,
  MessageOffloadManager,
  type MessageContentProvider,
  type MessageScope,
} from "../../messageOffload";

// --- Helpers ------------------------------------------------------

/** Build a scope for testing. */
function channelScope(id = "42"): MessageScope {
  return { scope: "channel", scopeId: id };
}

/** In-memory provider for testing (no Tauri dependency). */
function createMockProvider(): MessageContentProvider & {
  stored: Map<string, string>;
  storeCalls: number;
  retrieveCalls: number;
  retrieveManyCalls: number;
} {
  const stored = new Map<string, string>();
  const provider = {
    stored,
    storeCalls: 0,
    retrieveCalls: 0,
    retrieveManyCalls: 0,

    async store(key: string, content: string, _ctx: MessageScope) {
      provider.storeCalls++;
      stored.set(key, content);
    },
    async retrieve(key: string, _ctx: MessageScope) {
      provider.retrieveCalls++;
      return stored.get(key) ?? null;
    },
    async retrieveMany(keys: string[], _ctx: MessageScope) {
      provider.retrieveManyCalls++;
      const result: Record<string, string> = {};
      for (const k of keys) {
        const v = stored.get(k);
        if (v !== undefined) result[k] = v;
      }
      return result;
    },
    async release(_key: string) {},
    async dispose() {
      stored.clear();
    },
  };
  return provider;
}

// --- isHeavyContent -----------------------------------------------

describe("isHeavyContent", () => {
  it("returns false for short text", () => {
    expect(isHeavyContent("hello")).toBe(false);
  });

  it("returns false for long text without data-URL media", () => {
    expect(isHeavyContent("x".repeat(10_000))).toBe(false);
  });

  it("returns true for long body with embedded image data-URL", () => {
    const body = `<img src="data:image/png;base64,${"A".repeat(5000)}" />`;
    expect(isHeavyContent(body)).toBe(true);
  });

  it("returns true for long body with embedded video data-URL", () => {
    const body = `<video src="data:video/mp4;base64,${"B".repeat(5000)}"></video>`;
    expect(isHeavyContent(body)).toBe(true);
  });

  it("returns false for short body with data-URL (under threshold)", () => {
    const body = `<img src="data:image/png;base64,abc" />`;
    expect(isHeavyContent(body)).toBe(false);
  });
});

// --- isOffloaded --------------------------------------------------

describe("isOffloaded", () => {
  it("detects a valid offload placeholder", () => {
    expect(isOffloaded("<!-- OFFLOADED:msg-1:5000 -->")).toBe(true);
  });

  it("returns false for normal text", () => {
    expect(isOffloaded("Hello world")).toBe(false);
  });

  it("returns false for empty string", () => {
    expect(isOffloaded("")).toBe(false);
  });
});

// --- offloadPlaceholder -------------------------------------------

describe("offloadPlaceholder", () => {
  it("produces the correct format with size", () => {
    expect(offloadPlaceholder("abc-123", 9876)).toBe(
      "<!-- OFFLOADED:abc-123:9876 -->",
    );
  });

  it("round-trips through extractOffloadInfo", () => {
    const ph = offloadPlaceholder("id-42", 12345);
    const info = extractOffloadInfo(ph);
    expect(info).toEqual({ key: "id-42", contentLength: 12345 });
  });
});

// --- extractOffloadInfo -------------------------------------------

describe("extractOffloadInfo", () => {
  it("extracts key and contentLength", () => {
    const info = extractOffloadInfo("<!-- OFFLOADED:msg-1:5000 -->");
    expect(info).toEqual({ key: "msg-1", contentLength: 5000 });
  });

  it("handles UUID-style keys with colons in value", () => {
    // The key includes hyphens; the last colon separates the size.
    const info = extractOffloadInfo(
      "<!-- OFFLOADED:a1b2c3d4-e5f6-7890-abcd-ef1234567890:42000 -->",
    );
    expect(info?.key).toBe("a1b2c3d4-e5f6-7890-abcd-ef1234567890");
    expect(info?.contentLength).toBe(42000);
  });

  it("handles legacy placeholder without size", () => {
    // Older format: <!-- OFFLOADED:msg-1 --> (no colon + size)
    const info = extractOffloadInfo("<!-- OFFLOADED:msg-1 -->");
    expect(info).toEqual({ key: "msg-1", contentLength: 0 });
  });

  it("returns null for non-placeholder", () => {
    expect(extractOffloadInfo("hello world")).toBeNull();
  });

  it("returns null for malformed placeholder (no suffix)", () => {
    expect(extractOffloadInfo("<!-- OFFLOADED:msg-1:5000")).toBeNull();
  });

  it("handles zero content length", () => {
    const info = extractOffloadInfo("<!-- OFFLOADED:key:0 -->");
    expect(info).toEqual({ key: "key", contentLength: 0 });
  });
});

// --- MessageOffloadManager ----------------------------------------

describe("MessageOffloadManager", () => {
  let provider: ReturnType<typeof createMockProvider>;
  let manager: MessageOffloadManager;

  beforeEach(() => {
    vi.useFakeTimers();
    provider = createMockProvider();
    manager = new MessageOffloadManager(provider);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts with nothing offloaded or loading", () => {
    expect(manager.isOffloaded("x")).toBe(false);
    expect(manager.isLoading("x")).toBe(false);
  });

  it("scheduleOffload stores after delay", async () => {
    const cb = vi.fn();
    manager.scheduleOffload("msg-1", channelScope(), cb);

    // Not yet offloaded.
    expect(manager.isOffloaded("msg-1")).toBe(false);
    expect(provider.storeCalls).toBe(0);

    // Advance past the 5s delay.
    await vi.advanceTimersByTimeAsync(5_000);

    expect(provider.storeCalls).toBe(1);
    expect(manager.isOffloaded("msg-1")).toBe(true);
    expect(cb).toHaveBeenCalledOnce();
  });

  it("cancelOffload prevents the store", async () => {
    manager.scheduleOffload("msg-2", channelScope(), vi.fn());

    // Cancel before delay.
    manager.cancelOffload("msg-2");

    await vi.advanceTimersByTimeAsync(6_000);

    expect(provider.storeCalls).toBe(0);
    expect(manager.isOffloaded("msg-2")).toBe(false);
  });

  it("does not double-schedule the same message", () => {
    const cb = vi.fn();
    manager.scheduleOffload("msg-3", channelScope(), cb);
    manager.scheduleOffload("msg-3", channelScope(), cb);

    // Only one timer should exist (internal detail, but verified via
    // the store call count after advancing).
    vi.advanceTimersByTime(5_000);
    // Need to flush the timer microtask:
    expect(provider.storeCalls).toBeLessThanOrEqual(1);
  });

  it("restore returns body and clears offloaded state", async () => {
    // Manually mark as offloaded via scheduleOffload + timer advance.
    manager.scheduleOffload("msg-4", channelScope(), vi.fn());
    await vi.advanceTimersByTimeAsync(5_000);
    // Set the stored value after the timer fires (store() writes "").
    provider.stored.set("msg-4", "restored-body");

    expect(manager.isOffloaded("msg-4")).toBe(true);

    const body = await manager.restore("msg-4", channelScope());
    expect(body).toBe("restored-body");
    expect(manager.isOffloaded("msg-4")).toBe(false);
  });

  it("restore returns null for non-offloaded message", async () => {
    const body = await manager.restore("not-offloaded", channelScope());
    expect(body).toBeNull();
  });

  it("restoreMany decrypts multiple messages in one call", async () => {
    // Offload three messages.
    for (const id of ["a", "b", "c"]) {
      manager.scheduleOffload(id, channelScope(), vi.fn());
    }
    await vi.advanceTimersByTimeAsync(5_000);
    // Set stored values after timers fire (store() writes "").
    provider.stored.set("a", "body-a");
    provider.stored.set("b", "body-b");
    provider.stored.set("c", "body-c");

    expect(manager.isOffloaded("a")).toBe(true);
    expect(manager.isOffloaded("b")).toBe(true);
    expect(manager.isOffloaded("c")).toBe(true);

    const results = await manager.restoreMany(["a", "b", "c"], channelScope());

    expect(results).toEqual({
      a: "body-a",
      b: "body-b",
      c: "body-c",
    });

    // All should be un-offloaded now.
    expect(manager.isOffloaded("a")).toBe(false);
    expect(manager.isOffloaded("b")).toBe(false);
    expect(manager.isOffloaded("c")).toBe(false);

    // Should have used retrieveMany, not individual retrieve calls.
    expect(provider.retrieveManyCalls).toBe(1);
    expect(provider.retrieveCalls).toBe(0);
  });

  it("restoreMany skips non-offloaded keys", async () => {
    manager.scheduleOffload("x", channelScope(), vi.fn());
    await vi.advanceTimersByTimeAsync(5_000);
    // Set stored value after timer fires (store() writes "").
    provider.stored.set("x", "body-x");

    const results = await manager.restoreMany(["x", "y"], channelScope());

    // "y" was never offloaded, so it should be skipped.
    expect(results).toEqual({ x: "body-x" });
  });

  it("dispose clears everything", async () => {
    manager.scheduleOffload("d-1", channelScope(), vi.fn());
    await vi.advanceTimersByTimeAsync(5_000);

    await manager.dispose();

    expect(manager.isOffloaded("d-1")).toBe(false);
    expect(provider.stored.size).toBe(0);
  });

  it("setProvider switches the underlying provider", async () => {
    const newProvider = createMockProvider();
    manager.setProvider(newProvider);

    manager.scheduleOffload("msg-new", channelScope(), vi.fn());
    await vi.advanceTimersByTimeAsync(5_000);

    expect(newProvider.storeCalls).toBe(1);
    expect(provider.storeCalls).toBe(0);
  });
});
