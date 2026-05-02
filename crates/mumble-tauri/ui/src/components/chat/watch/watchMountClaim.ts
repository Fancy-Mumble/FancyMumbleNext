/**
 * Single-mount claim registry for watch-together sessions.
 *
 * A session can be rendered both from the `ActiveWatchBanner` above
 * the chat and from a `<!-- FANCY_WATCH:... -->` chat marker scrolled
 * into view.  Mounting the player adapter twice would race two embed
 * instances (and would double `join` traffic), so the first
 * `WatchTogetherCard` to render for a given session takes the
 * exclusive mount claim.  Late renderers render a placeholder
 * instead of the full player.
 *
 * Pure module state (no React) plus a `useSyncExternalStore` hook
 * for components that need to react to claim transitions.
 */

import { useSyncExternalStore } from "react";

const claims = new Map<string, string>();
const subscribers = new Set<() => void>();

function notify(): void {
  for (const sub of subscribers) sub();
}

/**
 * Try to claim exclusive mount rights for `sessionId`.  Returns true
 * when the caller now owns the claim (either freshly granted or
 * already owned).  No-op when another mount key holds the claim.
 */
export function claimWatchMount(sessionId: string, mountKey: string): boolean {
  const current = claims.get(sessionId);
  if (current === mountKey) return true;
  if (current != null) return false;
  claims.set(sessionId, mountKey);
  notify();
  return true;
}

/** Release the claim if held by `mountKey`. */
export function releaseWatchMount(sessionId: string, mountKey: string): void {
  if (claims.get(sessionId) !== mountKey) return;
  claims.delete(sessionId);
  notify();
}

/**
 * Subscribe to claim changes and read whether `mountKey` owns the
 * claim for `sessionId`.  When the claim is currently free this
 * returns false (the caller should attempt to claim it); when held by
 * another mount key it also returns false.
 */
export function useOwnsWatchMount(sessionId: string, mountKey: string): boolean {
  return useSyncExternalStore(
    (cb) => {
      subscribers.add(cb);
      return () => subscribers.delete(cb);
    },
    () => claims.get(sessionId) === mountKey,
    () => claims.get(sessionId) === mountKey,
  );
}

/** Test-only: clear all claims. */
export function _resetWatchMountClaimsForTests(): void {
  claims.clear();
  notify();
}
