/**
 * Regression tests for the lazy avatar / channel-description cache.
 *
 * Covers the contract that keeps the IPC payload small:
 *  - cached avatars are reused while `texture_size` matches
 *  - a changed `texture_size` triggers a re-fetch
 *  - empty / null sizes never invoke
 *  - `setUserAvatarBytes` populates the cache synchronously
 *  - in-flight requests for the same key are deduplicated
 */

import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(args[0] as string, args[1]),
}));

import {
  getCachedUserAvatar,
  getCachedChannelDescription,
  prefetchUserAvatar,
  prefetchChannelDescription,
  setUserAvatarBytes,
  _clearLazyBlobsForTests,
} from "../../lazyBlobs";

beforeEach(() => {
  invokeMock.mockReset();
  _clearLazyBlobsForTests();
});

describe("lazyBlobs avatar cache", () => {
  it("returns null for unknown sessions", () => {
    expect(getCachedUserAvatar(1, 100)).toBeNull();
  });

  it("returns null when textureSize is null or zero (no fetch needed)", () => {
    expect(getCachedUserAvatar(1, null)).toBeNull();
    expect(getCachedUserAvatar(1, 0)).toBeNull();
  });

  it("setUserAvatarBytes populates the cache synchronously", () => {
    setUserAvatarBytes(42, [1, 2, 3, 4]);
    const url = getCachedUserAvatar(42, 4);
    expect(url).not.toBeNull();
    expect(url!.startsWith("data:")).toBe(true);
  });

  it("returns null when the cached size does not match (stale -> refetch)", () => {
    setUserAvatarBytes(42, [1, 2, 3, 4]);
    expect(getCachedUserAvatar(42, 4)).not.toBeNull();
    expect(getCachedUserAvatar(42, 99)).toBeNull();
  });

  it("setUserAvatarBytes ignores empty bytes", () => {
    setUserAvatarBytes(7, null);
    setUserAvatarBytes(7, []);
    expect(getCachedUserAvatar(7, 0)).toBeNull();
  });

  it("prefetchUserAvatar invokes get_user_texture exactly once", async () => {
    invokeMock.mockResolvedValueOnce([10, 20, 30]);
    prefetchUserAvatar(5, 3);
    prefetchUserAvatar(5, 3); // dedup while in-flight
    await Promise.resolve();
    await Promise.resolve();
    const calls = invokeMock.mock.calls.filter((c) => c[0] === "get_user_texture");
    expect(calls).toHaveLength(1);
    expect(calls[0][1]).toEqual({ session: 5 });
  });

  it("prefetchUserAvatar does not invoke when already cached", async () => {
    setUserAvatarBytes(9, [1, 2]);
    prefetchUserAvatar(9, 2);
    await Promise.resolve();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("prefetchUserAvatar does not invoke for null/zero sizes", () => {
    prefetchUserAvatar(1, null);
    prefetchUserAvatar(1, 0);
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("lazyBlobs channel description cache", () => {
  it("returns null for unknown channels", () => {
    expect(getCachedChannelDescription(1, 50)).toBeNull();
  });

  it("returns null when descriptionSize is null or zero", () => {
    expect(getCachedChannelDescription(1, null)).toBeNull();
    expect(getCachedChannelDescription(1, 0)).toBeNull();
  });

  it("prefetchChannelDescription invokes get_channel_description and caches the text", async () => {
    invokeMock.mockResolvedValueOnce("hello world");
    prefetchChannelDescription(11, 11);
    await Promise.resolve();
    await Promise.resolve();
    const calls = invokeMock.mock.calls.filter((c) => c[0] === "get_channel_description");
    expect(calls).toHaveLength(1);
    expect(calls[0][1]).toEqual({ channelId: 11 });
    expect(getCachedChannelDescription(11, 11)).toBe("hello world");
  });

  it("does not re-invoke when description already cached", async () => {
    invokeMock.mockResolvedValueOnce("cached body");
    prefetchChannelDescription(20, 11);
    await Promise.resolve();
    await Promise.resolve();
    invokeMock.mockClear();

    prefetchChannelDescription(20, 11);
    await Promise.resolve();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("re-fetches when descriptionSize changes", async () => {
    invokeMock.mockResolvedValueOnce("first").mockResolvedValueOnce("second");
    prefetchChannelDescription(30, 5);
    await Promise.resolve();
    await Promise.resolve();
    expect(getCachedChannelDescription(30, 5)).toBe("first");

    prefetchChannelDescription(30, 6);
    await Promise.resolve();
    await Promise.resolve();
    expect(getCachedChannelDescription(30, 5)).toBeNull();
    expect(getCachedChannelDescription(30, 6)).toBe("second");
  });
});
