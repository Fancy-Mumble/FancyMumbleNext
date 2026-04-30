/**
 * Lazy-fetched user avatar and channel description blobs.
 *
 * The bulk endpoints `get_users` and `get_channels` only return the byte
 * length of these fields (`texture_size`, `description_size`) to keep the
 * IPC payload small.  Components that need to display the actual content
 * pull it on demand through these hooks.
 *
 * Caching strategy: keyed by `(id, size)`.  If the size changes (i.e. the
 * underlying blob was updated server-side) we re-fetch automatically.
 * The cache is bounded by an LRU eviction so long-running sessions do
 * not accumulate megabytes of stale data.
 */

import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { textureToDataUrl } from "./profileFormat";

interface CachedBlob<T> {
  size: number;
  value: T;
}

const CACHE_MAX = 200;
const avatarCache = new Map<number, CachedBlob<string>>();
const descriptionCache = new Map<number, CachedBlob<string>>();
const avatarPending = new Map<number, Promise<string | null>>();
const descriptionPending = new Map<number, Promise<string | null>>();

function lruTouch<T>(cache: Map<number, CachedBlob<T>>, key: number, entry: CachedBlob<T>): void {
  cache.delete(key);
  cache.set(key, entry);
  if (cache.size > CACHE_MAX) {
    const oldestKey = cache.keys().next().value;
    if (oldestKey !== undefined) cache.delete(oldestKey);
  }
}

/** Synchronously returns a cached avatar URL if present (and matches size). */
export function getCachedUserAvatar(session: number, textureSize: number | null): string | null {
  if (textureSize == null || textureSize === 0) return null;
  const cached = avatarCache.get(session);
  return cached && cached.size === textureSize ? cached.value : null;
}

/** Synchronously returns a cached description if present (and matches size). */
export function getCachedChannelDescription(channelId: number, descriptionSize: number | null): string | null {
  if (descriptionSize == null || descriptionSize === 0) return null;
  const cached = descriptionCache.get(channelId);
  return cached && cached.size === descriptionSize ? cached.value : null;
}

async function fetchUserAvatar(session: number, expectedSize: number): Promise<string | null> {
  const existing = avatarPending.get(session);
  if (existing) return existing;
  const promise = (async () => {
    try {
      const bytes = await invoke<number[] | null>("get_user_texture", { session });
      if (!bytes || bytes.length === 0) return null;
      const url = textureToDataUrl(bytes);
      lruTouch(avatarCache, session, { size: expectedSize, value: url });
      return url;
    } catch (e) {
      console.error("get_user_texture failed", session, e);
      return null;
    } finally {
      avatarPending.delete(session);
    }
  })();
  avatarPending.set(session, promise);
  return promise;
}

async function fetchChannelDescription(channelId: number, expectedSize: number): Promise<string | null> {
  const existing = descriptionPending.get(channelId);
  if (existing) return existing;
  const promise = (async () => {
    try {
      const text = await invoke<string | null>("get_channel_description", { channelId });
      if (!text) return null;
      lruTouch(descriptionCache, channelId, { size: expectedSize, value: text });
      return text;
    } catch (e) {
      console.error("get_channel_description failed", channelId, e);
      return null;
    } finally {
      descriptionPending.delete(channelId);
    }
  })();
  descriptionPending.set(channelId, promise);
  return promise;
}

/** React hook: returns the avatar data-URL for a user, or `null` while loading or unset. */
export function useUserAvatar(session: number | null | undefined, textureSize: number | null | undefined): string | null {
  const initial = session != null && textureSize != null
    ? getCachedUserAvatar(session, textureSize)
    : null;
  const [url, setUrl] = useState<string | null>(initial);

  useEffect(() => {
    if (session == null || textureSize == null || textureSize === 0) {
      setUrl(null);
      return;
    }
    const cached = getCachedUserAvatar(session, textureSize);
    if (cached) {
      setUrl(cached);
      return;
    }
    setUrl(null);
    let cancelled = false;
    fetchUserAvatar(session, textureSize).then((u) => {
      if (!cancelled) setUrl(u);
    });
    return () => {
      cancelled = true;
    };
  }, [session, textureSize]);

  return url;
}

/** React hook: returns the description text for a channel, or `null` while loading or empty. */
export function useChannelDescription(
  channelId: number | null | undefined,
  descriptionSize: number | null | undefined,
): string | null {
  const initial = channelId != null && descriptionSize != null
    ? getCachedChannelDescription(channelId, descriptionSize)
    : null;
  const [text, setText] = useState<string | null>(initial);

  useEffect(() => {
    if (channelId == null || descriptionSize == null || descriptionSize === 0) {
      setText(null);
      return;
    }
    const cached = getCachedChannelDescription(channelId, descriptionSize);
    if (cached) {
      setText(cached);
      return;
    }
    setText(null);
    let cancelled = false;
    fetchChannelDescription(channelId, descriptionSize).then((t) => {
      if (!cancelled) setText(t);
    });
    return () => {
      cancelled = true;
    };
  }, [channelId, descriptionSize]);

  return text;
}

/** Imperatively prefetch an avatar (for use outside React, e.g. from a store action). */
export function prefetchUserAvatar(session: number, textureSize: number | null): void {
  if (textureSize == null || textureSize === 0) return;
  if (getCachedUserAvatar(session, textureSize)) return;
  void fetchUserAvatar(session, textureSize);
}

/**
 * Synchronously install raw avatar bytes (e.g. from `UserList` admin
 * response, where the bytes are sent inline) into the cache so that
 * `useUserAvatar(session, bytes.length)` resolves without an IPC call.
 */
export function setUserAvatarBytes(session: number, bytes: number[] | null): void {
  if (!bytes || bytes.length === 0) return;
  const url = textureToDataUrl(bytes);
  lruTouch(avatarCache, session, { size: bytes.length, value: url });
}

/**
 * React hook: returns `Map<session, dataUrl>` for many users at once.
 * Triggers lazy fetches for any user whose avatar isn't cached yet, then
 * re-renders as each one resolves.  Cheaper than mounting many
 * `useUserAvatar` hooks for hundreds of message rows.
 */
export function useUserAvatars(
  users: ReadonlyArray<{ session: number; texture_size: number | null }>,
): Map<number, string> {
  const [version, bump] = useState(0);

  useEffect(() => {
    let cancelled = false;
    for (const u of users) {
      if (u.texture_size == null || u.texture_size === 0) continue;
      if (getCachedUserAvatar(u.session, u.texture_size)) continue;
      void fetchUserAvatar(u.session, u.texture_size).then(() => {
        if (!cancelled) bump((v) => v + 1);
      });
    }
    return () => {
      cancelled = true;
    };
  }, [users]);

  return useMemo(() => {
    void version;
    const map = new Map<number, string>();
    for (const u of users) {
      const cached = getCachedUserAvatar(u.session, u.texture_size);
      if (cached) map.set(u.session, cached);
    }
    return map;
  }, [users, version]);
}

/** Imperatively prefetch a channel description. */
export function prefetchChannelDescription(channelId: number, descriptionSize: number | null): void {
  if (descriptionSize == null || descriptionSize === 0) return;
  if (getCachedChannelDescription(channelId, descriptionSize)) return;
  void fetchChannelDescription(channelId, descriptionSize);
}

/** Test helper: clear all caches. */
export function _clearLazyBlobsForTests(): void {
  avatarCache.clear();
  descriptionCache.clear();
  avatarPending.clear();
  descriptionPending.clear();
}
