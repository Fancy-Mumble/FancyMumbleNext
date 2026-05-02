/**
 * Tracks watch-together sessions that were *just created* by the
 * local user.  The watch-together card uses this to auto-start
 * playback once the underlying player adapter is ready, which makes
 * the start symmetric: the originator sees the video play just like
 * everyone else does (the host's local play event then propagates
 * over the wire as a `state: playing` update).
 *
 * The flag is consumed exactly once - subsequent mounts of the same
 * session render normally without auto-starting.
 */

const pending = new Set<string>();

export function markPendingAutoStart(sessionId: string): void {
  pending.add(sessionId);
}

/**
 * Returns true and clears the flag if `sessionId` was previously
 * marked.  Subsequent calls return false.
 */
export function consumePendingAutoStart(sessionId: string): boolean {
  if (!pending.has(sessionId)) return false;
  pending.delete(sessionId);
  return true;
}

/** Test-only reset. */
export function _resetPendingAutoStartForTests(): void {
  pending.clear();
}
