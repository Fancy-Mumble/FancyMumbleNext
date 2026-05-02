/**
 * `ActiveWatchBanner` lists watch-together sessions active in the
 * channel the local user is currently in.  This is the discovery
 * surface for users who joined the channel after the original
 * `<!-- FANCY_WATCH:... -->` chat marker scrolled out of view (or
 * was never in their history).
 *
 * Clicking "Open" expands the entry into a full `WatchTogetherCard`.
 * The single-mount registry in `watchMountClaim` makes sure no
 * second card mounts the player adapter for the same session at the
 * same time.
 */

import { memo, useEffect, useMemo, useState } from "react";

import { useAppStore } from "../../../store";
import WatchTogetherCard from "./WatchTogetherCard";
import { claimWatchMount, releaseWatchMount } from "./watchMountClaim";
import styles from "./WatchTogetherCard.module.css";

const BANNER_OWNER_PREFIX = "banner:";

function ActiveWatchBannerImpl() {
  const sessions = useAppStore((s) => s.watchSessions);
  const currentChannel = useAppStore((s) => s.currentChannel);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const [openSessionId, setOpenSessionId] = useState<string | null>(null);

  const inChannel = useMemo(() => {
    if (currentChannel == null || selectedChannel !== currentChannel) return [];
    const out = [];
    for (const session of sessions.values()) {
      if (session.channelId === currentChannel) out.push(session);
    }
    return out;
  }, [sessions, currentChannel, selectedChannel]);

  // If the open session disappears (ended/host gone), collapse.
  useEffect(() => {
    if (openSessionId == null) return;
    if (!sessions.has(openSessionId)) setOpenSessionId(null);
  }, [openSessionId, sessions]);

  if (inChannel.length === 0) return null;

  return (
    <div>
      {inChannel.map((session) => {
        if (openSessionId === session.sessionId) {
          return (
            <ExpandedSession
              key={session.sessionId}
              sessionId={session.sessionId}
              onClose={() => setOpenSessionId(null)}
            />
          );
        }
        return (
          <CollapsedSession
            key={session.sessionId}
            title={session.title ?? session.sourceUrl}
            participants={session.participants.size}
            onOpen={() => setOpenSessionId(session.sessionId)}
          />
        );
      })}
    </div>
  );
}

interface CollapsedProps {
  readonly title: string;
  readonly participants: number;
  readonly onOpen: () => void;
}

function CollapsedSession({ title, participants, onOpen }: CollapsedProps) {
  return (
    <div className={styles.card}>
      <div className={styles.header}>
        <span className={styles.title}>{title}</span>
        <span className={styles.badges}>
          <span className={styles.participants}>{participants} watching</span>
        </span>
      </div>
      <div className={styles.actions}>
        <button type="button" onClick={onOpen}>Open</button>
      </div>
    </div>
  );
}

interface ExpandedProps {
  readonly sessionId: string;
  readonly onClose: () => void;
}

function ExpandedSession({ sessionId, onClose }: ExpandedProps) {
  // Take the mount claim for the lifetime of this expanded view so
  // any concurrent chat-marker render falls back to a placeholder.
  useEffect(() => {
    const ownerId = `${BANNER_OWNER_PREFIX}${sessionId}`;
    claimWatchMount(sessionId, ownerId);
    return () => releaseWatchMount(sessionId, ownerId);
  }, [sessionId]);

  return (
    <div>
      <div className={styles.actions} style={{ justifyContent: "flex-end" }}>
        <button type="button" onClick={onClose}>Collapse</button>
      </div>
      <WatchTogetherCard sessionId={sessionId} mountKey={`${BANNER_OWNER_PREFIX}${sessionId}`} />
    </div>
  );
}

const ActiveWatchBanner = memo(ActiveWatchBannerImpl);
export default ActiveWatchBanner;
