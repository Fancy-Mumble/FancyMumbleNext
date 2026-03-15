import { useCallback, useRef, useEffect } from "react";
import { useAppStore } from "../store";
import type { PersistenceMode } from "../types";
import styles from "./PersistenceBanner.module.css";

interface PersistenceBannerProps {
  readonly channelId: number;
}

function modeDescription(mode: PersistenceMode): string {
  switch (mode) {
    case "POST_JOIN":
      return "Messages are visible from the moment you first joined this channel.";
    case "FULL_ARCHIVE":
      return "All stored messages are visible to channel members.";
    default:
      return "";
  }
}

function formatRetention(days: number): string {
  if (days <= 0) return "No limit";
  if (days === 1) return "1 day";
  return `${days} days`;
}

function formatCount(count: number): string {
  if (count >= 1000) return `${(count / 1000).toFixed(1)}k`;
  return String(count);
}

export default function PersistenceBanner({ channelId }: PersistenceBannerProps) {
  const persistence = useAppStore((s) => s.channelPersistence[channelId]);
  const fetchHistory = useAppStore((s) => s.fetchHistory);

  // Intersection observer for "load more" scroll-to-top trigger.
  const loadMoreRef = useRef<HTMLDivElement>(null);

  const handleLoadMore = useCallback(() => {
    if (!persistence || persistence.isFetching || !persistence.hasMore) return;
    const messages = useAppStore.getState().messages;
    const firstId = messages.length > 0 ? messages[0].message_id : undefined;
    fetchHistory(channelId, firstId ?? undefined);
  }, [channelId, fetchHistory, persistence]);

  useEffect(() => {
    const el = loadMoreRef.current;
    if (!el || !persistence?.hasMore) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting) handleLoadMore();
      },
      { threshold: 0.1 },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [persistence?.hasMore, handleLoadMore]);

  if (!persistence || persistence.mode === "NONE") return null;

  return (
    <>
      <div className={styles.banner}>
        {/* Shield/lock icon */}
        <svg className={styles.icon} viewBox="0 0 24 24" fill="none" stroke="currentColor"
          strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
        </svg>
        <div className={styles.content}>
          <p className={styles.description}>
            {modeDescription(persistence.mode)}
          </p>
          <div className={styles.meta}>
            {persistence.retentionDays > 0 && (
              <span className={styles.metaItem}>
                Retention: {formatRetention(persistence.retentionDays)}
              </span>
            )}
            {persistence.totalStored > 0 && (
              <span className={styles.metaItem}>
                Stored: {formatCount(persistence.totalStored)} messages
              </span>
            )}
          </div>
        </div>
      </div>

      {/* Invisible sentinel for intersection-observer-based pagination */}
      {persistence.hasMore && (
        <div ref={loadMoreRef} className={styles.loadMore}>
          {persistence.isFetching && (
            <div className={styles.loadingSpinner} aria-label="Loading history" />
          )}
        </div>
      )}
    </>
  );
}
