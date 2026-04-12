import { useCallback, useRef, useEffect, useState } from "react";
import { useAppStore } from "../../store";
import type { PersistenceMode } from "../../types";
import { getDismissedBanners, dismissBanner } from "../../preferencesStorage";
import { InfoBanner } from "./InfoBanner";
import ShieldIcon from "../../assets/icons/status/shield.svg?react";
import styles from "./InfoBanner.module.css";

interface PersistenceBannerProps {
  readonly channelId: number;
}

function modeDescription(mode: PersistenceMode): string {
  switch (mode) {
    case "FANCY_V1_FULL_ARCHIVE":
      return "All stored messages are visible to channel members.";
    case "SIGNAL_V1":
      return "Messages are end-to-end encrypted using the Signal Protocol.";
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
  const isLoadingKeys = useAppStore((s) => s.pchatHistoryLoading.has(channelId));
  const [dismissed, setDismissed] = useState(false);

  // Load persisted dismissal state on channel change.
  useEffect(() => {
    let cancelled = false;
    getDismissedBanners().then((ids) => {
      if (!cancelled) setDismissed(ids.includes(channelId));
    });
    return () => { cancelled = true; };
  }, [channelId]);

  const handleDismiss = useCallback(() => {
    setDismissed(true);
    dismissBanner(channelId);
  }, [channelId]);

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

  const shieldIcon = <ShieldIcon aria-hidden="true" />;

  // Show loading indicator even before persistence config is known.
  if (isLoadingKeys && (!persistence || persistence.mode === "NONE")) {
    return (
      <div className={styles.loadMore}>
        <div className={styles.loadingSpinner} aria-label="Loading message history" />
        <span className={styles.loadingText}>Loading message history...</span>
      </div>
    );
  }

  if (!persistence || persistence.mode === "NONE") return null;

  return (
    <>
      {!dismissed && (
        <InfoBanner icon={shieldIcon} onDismiss={handleDismiss}>
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
        </InfoBanner>
      )}

      {/* Key exchange / initial loading indicator */}
      {isLoadingKeys && (
        <div className={styles.loadMore}>
          <div className={styles.loadingSpinner} aria-label="Loading message history" />
          <span className={styles.loadingText}>Loading message history...</span>
        </div>
      )}

      {/* Invisible sentinel for intersection-observer-based pagination */}
      {persistence.hasMore && (
        <div ref={loadMoreRef} className={styles.loadMore}>
          {persistence.isFetching && (
            <div className={styles.loadingSpinner} aria-label="Loading older messages" />
          )}
        </div>
      )}
    </>
  );
}
