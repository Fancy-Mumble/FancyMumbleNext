/**
 * Lifecycle effects for watch-together sessions.
 *
 * - **Host re-election**: when the current host disappears from the
 *   user list (disconnect, kick, ban) every remaining participant
 *   computes the same deterministic hash over the candidate session
 *   IDs and the session UUID; the lowest hash wins.  Only the winner
 *   actually sends a `hostTransfer` event so all peers converge on
 *   the same new host without coordination.
 *
 * - **Participant pruning**: a remote user disappearing from the
 *   server user list (disconnect/kick/move-out-of-channel) does not
 *   trigger a `leave` event.  Every client locally prunes missing
 *   sessions from each `WatchSession.participants` set so the "N
 *   watching" badge stays accurate.  This is purely local mutation
 *   and converges deterministically because all clients see the same
 *   user list.
 *
 * - **Host re-advertise**: hosts re-broadcast `start` when a new
 *   channel member appears so newcomers can populate their local
 *   `watchSessions` map without history.
 *
 * - **Channel switch leave**: when the local user moves to a different
 *   channel, send `leave` for every active session bound to the
 *   channel they are leaving.  Hosts additionally send `end` to tear
 *   down the session for everyone (we cannot remain authoritative
 *   from a different channel).
 *
 * Mount once at the App level.
 */

import { useEffect, useRef } from "react";

import { useAppStore } from "../../../store";
import { applyWatchSyncEvent } from "./watchStore";
import { useWatchSend } from "./useWatchSend";
import type { WatchSession } from "./watchTypes";

export function useWatchLifecycle(): void {
  const sessions = useAppStore((s) => s.watchSessions);
  const users = useAppStore((s) => s.users);
  const ownSession = useAppStore((s) => s.ownSession);
  const currentChannel = useAppStore((s) => s.currentChannel);
  const { sendHostTransfer, sendLeave, sendEnd, sendStart, sendState } = useWatchSend();

  // -- Host re-election ---------------------------------------------
  // Memoise which (sessionId, hostSession) pairs we've already
  // attempted to re-elect for so we don't fire the transfer twice if
  // the user list updates rapidly.
  const reelectedRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (ownSession == null) return;
    const presentSessions = new Set(users.map((u) => u.session));
    for (const session of sessions.values()) {
      if (!session.participants.has(ownSession)) continue;
      if (presentSessions.has(session.hostSession)) continue;
      const dedupKey = `${session.sessionId}:${session.hostSession}`;
      if (reelectedRef.current.has(dedupKey)) continue;
      const candidates = [...session.participants].filter(
        (s) => s !== session.hostSession && presentSessions.has(s),
      );
      if (candidates.length === 0) continue;
      const winner = electHost(candidates, session.sessionId);
      reelectedRef.current.add(dedupKey);
      if (winner === ownSession) {
        void sendHostTransfer(session.sessionId, ownSession);
      }
    }
  }, [sessions, users, ownSession, sendHostTransfer]);

  // -- Channel switch leave -----------------------------------------
  const lastChannelRef = useRef<number | null>(currentChannel);
  useEffect(() => {
    const previous = lastChannelRef.current;
    lastChannelRef.current = currentChannel;
    if (previous == null || previous === currentChannel) return;
    if (ownSession == null) return;
    for (const session of sessions.values()) {
      if (session.channelId !== previous) continue;
      if (!session.participants.has(ownSession)) continue;
      if (session.hostSession === ownSession) {
        void sendEnd(session.sessionId);
      } else {
        void sendLeave(session.sessionId, ownSession);
      }
    }
    // Intentionally not depending on `sessions` so this only fires on
    // an actual channel transition - sessions changing within the
    // same channel must not trigger a leave.
  }, [currentChannel]);

  // -- Participant pruning -------------------------------------------
  // Remove participants that no longer exist in the user list (i.e.
  // disconnected / kicked / moved to another channel).  All clients
  // see the same `users` list so this converges deterministically
  // without any wire chatter.  Pure local mutation via the same
  // reducer the wire path uses, to keep behaviour identical.
  useEffect(() => {
    const presentSessions = new Set(users.map((u) => u.session));
    for (const session of sessions.values()) {
      for (const participant of session.participants) {
        if (presentSessions.has(participant)) continue;
        applyWatchSyncEvent({
          sessionId: session.sessionId,
          actor: participant,
          event: { type: "leave", session: participant },
        });
      }
    }
  }, [sessions, users]);

  // -- Host re-advertise on new channel members ----------------------
  // When a new user appears in a channel where we host an active
  // session, re-broadcast `start` (and a fresh `state` snapshot) so
  // the newcomer can render the session immediately.  We track the
  // previously-seen session IDs per channel and fire only on
  // additions to avoid resending on every users-list update.
  const seenInChannelRef = useRef<Map<number, Set<number>>>(new Map());
  useEffect(() => {
    if (ownSession == null) return;
    const byChannel = new Map<number, Set<number>>();
    for (const u of users) {
      const set = byChannel.get(u.channel_id) ?? new Set<number>();
      set.add(u.session);
      byChannel.set(u.channel_id, set);
    }
    for (const session of sessions.values()) {
      if (session.hostSession !== ownSession) continue;
      const present = byChannel.get(session.channelId);
      if (!present) continue;
      const previouslySeen = seenInChannelRef.current.get(session.channelId) ?? new Set<number>();
      const newcomers: number[] = [];
      for (const s of present) {
        if (s === ownSession) continue;
        if (!previouslySeen.has(s)) newcomers.push(s);
      }
      if (newcomers.length === 0) continue;
      void sendStart(session.sessionId, {
        type: "start",
        channelId: session.channelId,
        sourceUrl: session.sourceUrl,
        sourceKind: session.sourceKind,
        title: session.title,
        hostSession: session.hostSession,
      });
      void sendState(session.sessionId, {
        type: "state",
        state: session.state,
        currentTime: session.currentTime,
        updatedAtMs: Date.now(),
        hostSession: session.hostSession,
      });
    }
    seenInChannelRef.current = byChannel;
  }, [users, sessions, ownSession, sendStart, sendState]);
}

/**
 * Deterministic host election.  All clients pass the same candidates
 * and session UUID and obtain the same winner.
 */
export function electHost(candidates: readonly number[], sessionId: string): number | null {
  if (candidates.length === 0) return null;
  let best: { hash: number; session: number } | null = null;
  for (const session of candidates) {
    const hash = fnv1a(`${session}:${sessionId}`);
    if (best === null || hash < best.hash || (hash === best.hash && session < best.session)) {
      best = { hash, session };
    }
  }
  return best?.session ?? null;
}

/** 32-bit FNV-1a hash. */
function fnv1a(input: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

/** Test-only export for inspecting session state. */
export type _WatchSessionForTests = WatchSession;
