/**
 * Unit tests for silenced channels persistence (preferencesStorage).
 *
 * Verifies that channels can be silenced/unsilenced per server key,
 * and that the data is properly persisted and isolated between servers.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";

// In-memory store backing for the mock.
let storeData: Record<string, unknown> = {};

vi.mock("@tauri-apps/plugin-store", () => ({
  load: vi.fn().mockImplementation(() =>
    Promise.resolve({
      get: vi.fn().mockImplementation((key: string) =>
        Promise.resolve(storeData[key] ?? null),
      ),
      set: vi.fn().mockImplementation((key: string, value: unknown) => {
        storeData[key] = value;
        return Promise.resolve();
      }),
    }),
  ),
}));

// Import after mocks are in place.
import {
  getSilencedChannels,
  setSilencedChannel,
} from "../../preferencesStorage";

describe("Silenced channels storage", () => {
  beforeEach(() => {
    storeData = {};
  });

  it("returns empty array for unknown server", async () => {
    const result = await getSilencedChannels("example.com:64738");
    expect(result).toEqual([]);
  });

  it("silences a channel and persists it", async () => {
    const updated = await setSilencedChannel("example.com:64738", 42, true);
    expect(updated).toEqual([42]);

    const stored = await getSilencedChannels("example.com:64738");
    expect(stored).toEqual([42]);
  });

  it("does not duplicate when silencing the same channel twice", async () => {
    await setSilencedChannel("example.com:64738", 42, true);
    const second = await setSilencedChannel("example.com:64738", 42, true);
    expect(second).toEqual([42]);
  });

  it("unsilences a channel", async () => {
    await setSilencedChannel("example.com:64738", 42, true);
    await setSilencedChannel("example.com:64738", 10, true);
    const updated = await setSilencedChannel("example.com:64738", 42, false);
    expect(updated).toEqual([10]);
  });

  it("unsilencing a non-silenced channel is a no-op", async () => {
    const updated = await setSilencedChannel("example.com:64738", 99, false);
    expect(updated).toEqual([]);
  });

  it("isolates silenced channels per server", async () => {
    await setSilencedChannel("server-a.com:64738", 1, true);
    await setSilencedChannel("server-b.com:64738", 2, true);

    const a = await getSilencedChannels("server-a.com:64738");
    const b = await getSilencedChannels("server-b.com:64738");
    expect(a).toEqual([1]);
    expect(b).toEqual([2]);
  });

  it("silences multiple channels on the same server", async () => {
    await setSilencedChannel("example.com:64738", 1, true);
    await setSilencedChannel("example.com:64738", 2, true);
    await setSilencedChannel("example.com:64738", 3, true);

    const result = await getSilencedChannels("example.com:64738");
    expect(result).toEqual([1, 2, 3]);
  });
});
