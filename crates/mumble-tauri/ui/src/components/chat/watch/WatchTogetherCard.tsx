/**
 * `WatchTogetherCard` renders an active watch-together session
 * inline in the chat.
 *
 * Looks the session up by ID from `useAppStore`, mounts a player
 * adapter into a ref, and delegates synchronisation to
 * `useWatchSync`.  Fails gracefully (renders an info banner) when:
 *
 * - The session is not (yet) known locally.
 * - The session is for a different channel than the one the message
 *   appears in.
 * - The source kind is `youtube` but the user has not opted in to
 *   external embeds.
 */

import { memo, useCallback, useEffect, useId, useMemo, useRef, useState } from "react";

import { useAppStore } from "../../../store";
import { createPlayerAdapter } from "./createPlayerAdapter";
import type { PlayerAdapter } from "./PlayerAdapter";
import { useWatchSend } from "./useWatchSend";
import { useWatchSync } from "./useWatchSync";
import { applyWatchSyncEvent } from "./watchStore";
import { consumePendingAutoStart } from "./watchAutoStart";
import { claimWatchMount, releaseWatchMount, useOwnsWatchMount } from "./watchMountClaim";
import styles from "./WatchTogetherCard.module.css";

interface Props {
  readonly sessionId: string;
  /**
   * Stable identifier for the mount instance.  When omitted a unique
   * id is generated.  The first card to render for a given
   * `sessionId` claims the player mount; later cards render a
   * placeholder so the underlying adapter is not mounted twice.
   */
  readonly mountKey?: string;
}

function WatchTogetherCardImpl({ sessionId, mountKey }: Props) {
  const session = useAppStore((s) => s.watchSessions.get(sessionId));
  const ownSession = useAppStore((s) => s.ownSession);
  const enableExternalEmbeds = useAppStore((s) => s.enableExternalEmbeds);
  const users = useAppStore((s) => s.users);
  const { sendJoin, sendState } = useWatchSend();

  const generatedKey = useId();
  const effectiveMountKey = mountKey ?? generatedKey;
  const owns = useOwnsWatchMount(sessionId, effectiveMountKey);
  // Try to claim the slot every render; `claimWatchMount` no-ops when
  // already held by us or by another mount key.  Release on unmount
  // so a sibling card can take over.
  useEffect(() => {
    claimWatchMount(sessionId, effectiveMountKey);
    return () => releaseWatchMount(sessionId, effectiveMountKey);
  }, [sessionId, effectiveMountKey]);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const [adapter, setAdapter] = useState<PlayerAdapter | null>(null);
  const [adapterError, setAdapterError] = useState<string | null>(null);
  // Tracks an explicit user-initiated `leave`.  Without this the
  // auto-join effect below would immediately rejoin the session,
  // making the Leave button visually a no-op.  Reset whenever the
  // session ID changes so opening a different session works.
  const [explicitlyLeft, setExplicitlyLeft] = useState(false);
  useEffect(() => {
    setExplicitlyLeft(false);
  }, [sessionId]);
  const sourceKind = session?.sourceKind;
  const sourceUrl = session?.sourceUrl;

  // Mount the player whenever the source changes.  Skip mounting
  // entirely once the user has left so we stop pulling state and
  // free the embed.  Also skip when another card owns the mount
  // claim for this session.
  useEffect(() => {
    if (!sourceKind || !sourceUrl || explicitlyLeft || !owns) return;
    const container = containerRef.current;
    if (!container) return;
    let next: PlayerAdapter | null = null;
    try {
      next = createPlayerAdapter(
        sourceKind,
        { container, sourceUrl },
        enableExternalEmbeds,
      );
      setAdapter(next);
      setAdapterError(null);
    } catch (err) {
      setAdapter(null);
      setAdapterError(err instanceof Error ? err.message : String(err));
    }
    return () => {
      next?.destroy();
      setAdapter(null);
    };
  }, [sourceKind, sourceUrl, enableExternalEmbeds, explicitlyLeft, owns]);

  // Send a `join` event the first time we render for a session we are
  // not already part of.  Suppressed while `explicitlyLeft` so the
  // Leave button is sticky.  Only the owning mount sends the join so
  // we don't double up when both banner and chat marker render.
  useEffect(() => {
    if (!session || ownSession == null) return;
    if (explicitlyLeft || !owns) return;
    if (session.participants.has(ownSession)) return;
    void sendJoin(sessionId, ownSession);
    // Optimistic local apply: server does not echo events back to the
    // sender, so without this our own participant count would lag
    // until the next remote event.
    applyWatchSyncEvent({
      sessionId,
      actor: ownSession,
      event: { type: "join", session: ownSession },
    });
  }, [session, sessionId, ownSession, sendJoin, explicitlyLeft, owns]);

  // useWatchSync is always called (even when session is undefined) to
  // keep hook order stable; it returns no-op handlers in that case.
  const safeSession = session ?? {
    sessionId,
    channelId: -1,
    hostSession: -1,
    sourceUrl: "",
    sourceKind: "directMedia" as const,
    participants: new Set<number>(),
    state: "paused" as const,
    currentTime: 0,
    updatedAtMs: 0,
  };
  const { isHost, outOfSync, requestState, leave, end } = useWatchSync({
    adapter,
    session: safeSession,
    ownSession,
  });

  // Auto-start playback for the originator: when this user just
  // started the session and the adapter has finished mounting, kick
  // off `play(0)` and broadcast `state: playing` so non-hosts seek
  // and play in lockstep.  We have to send the state explicitly
  // because `adapter.play` suppresses local events to prevent loops.
  useEffect(() => {
    if (!adapter || !isHost || !owns) return;
    if (ownSession == null) return;
    if (!consumePendingAutoStart(sessionId)) return;
    void (async () => {
      await adapter.play(0);
      await sendState(sessionId, {
        type: "state",
        state: "playing",
        currentTime: 0,
        updatedAtMs: Date.now(),
        hostSession: ownSession,
      });
    })();
  }, [adapter, isHost, owns, sessionId, ownSession, sendState]);

  const handleLeave = useCallback(async () => {
    setExplicitlyLeft(true);
    if (ownSession != null) {
      // Optimistic local apply: server does not echo to sender, so
      // without this the participant count would not drop until the
      // next remote event arrives.
      applyWatchSyncEvent({
        sessionId,
        actor: ownSession,
        event: { type: "leave", session: ownSession },
      });
    }
    await leave();
  }, [leave, ownSession, sessionId]);

  const handleRejoin = useCallback(() => {
    setExplicitlyLeft(false);
  }, []);

  const handleEnd = useCallback(async () => {
    if (ownSession != null) {
      // Optimistic local apply: removes the session from our store
      // so our card stops rendering as if the session were live.
      applyWatchSyncEvent({
        sessionId,
        actor: ownSession,
        event: { type: "end" },
      });
    }
    await end();
  }, [end, ownSession, sessionId]);

  const hostName = useMemo(() => {
    if (!session) return null;
    return users.find((u) => u.session === session.hostSession)?.name ?? `#${session.hostSession}`;
  }, [users, session]);

  if (!session) {
    // Session has ended (or never existed for us) - render nothing
    // so the chat marker visually disappears.  We still completed
    // any prior cleanup via the effects above.
    return null;
  }

  if (!owns) {
    return (
      <div className={styles.card}>
        <div className={styles.header}>
          <span className={styles.title}>{session.title ?? session.sourceUrl}</span>
          <span className={styles.badges}>
            <span className={styles.participants}>
              {session.participants.size} watching{hostName ? ` \u00B7 host: ${hostName}` : ""}
            </span>
          </span>
        </div>
        <div className={styles.warning}>
          Watch session is open elsewhere on this page.
        </div>
      </div>
    );
  }

  if (explicitlyLeft) {
    return (
      <div className={styles.card}>
        <div className={styles.header}>
          <span className={styles.title}>{session.title ?? session.sourceUrl}</span>
          <span className={styles.badges}>
            <span className={styles.participants}>
              {session.participants.size} watching{hostName ? ` \u00B7 host: ${hostName}` : ""}
            </span>
          </span>
        </div>
        <div className={styles.actions}>
          <button type="button" onClick={handleRejoin}>Rejoin</button>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.card}>
      <div className={styles.header}>
        <span className={styles.title}>{session.title ?? session.sourceUrl}</span>
        <span className={styles.badges}>
          {isHost && <span className={styles.hostBadge}>HOST</span>}
          <span className={styles.participants}>
            {session.participants.size} watching{hostName ? ` \u00B7 host: ${hostName}` : ""}
          </span>
        </span>
      </div>

      <div ref={containerRef} className={styles.player} />

      {adapterError && <div className={styles.error}>{adapterError}</div>}
      {outOfSync && (
        <div className={styles.warning}>
          Out of sync with host. <button type="button" onClick={() => void requestState()}>Resync</button>
        </div>
      )}

      <div className={styles.actions}>
        <button type="button" onClick={() => void requestState()}>Request state</button>
        <button type="button" className={styles.danger} onClick={() => void handleLeave()}>Leave</button>
        {isHost && (
          <button type="button" className={styles.danger} onClick={() => void handleEnd()}>End for everyone</button>
        )}
      </div>
    </div>
  );
}

const WatchTogetherCard = memo(WatchTogetherCardImpl);
export default WatchTogetherCard;
