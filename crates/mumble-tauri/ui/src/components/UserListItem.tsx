import { useState, useMemo, useCallback, useRef } from "react";
import { createPortal } from "react-dom";
import { useAppStore } from "../store";
import type { UserEntry } from "../types";
import { textureToDataUrl, parseComment } from "../profileFormat";
import { ProfilePreviewCard } from "../pages/settings/ProfilePreviewCard";
import { useUserStats } from "../hooks/useUserStats";
import { colorFor } from "../utils/format";
import { isMobilePlatform } from "../utils/platform";
import styles from "./UserListItem.module.css";

// Re-export so existing consumers (e.g. ChannelSidebar) keep working.
export { colorFor } from "../utils/format";

const textureCache = new Map<number, { len: number; url: string }>();

export function avatarUrl(user: UserEntry): string | null {
  if (!user.texture || user.texture.length === 0) return null;
  const cached = textureCache.get(user.session);
  if (cached?.len === user.texture.length) return cached.url;
  const url = textureToDataUrl(user.texture);
  textureCache.set(user.session, { len: user.texture.length, url });
  return url;
}

// -- Constants -----------------------------------------------------

const HOVER_CARD_W = 260;
const HOVER_CARD_H = 340;
const HOVER_CARD_MARGIN = 10;
const HOVER_CARD_GAP = 8;

// -- SVG icons -----------------------------------------------------

function MutedIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="1" y1="1" x2="23" y2="23" />
      <path d="M9 9v3a3 3 0 0 0 5.12 2.12M15 9.34V4a3 3 0 0 0-5.94-.6" />
      <path d="M17 16.95A7 7 0 0 1 5 12v-2m14 0v2c0 .76-.13 1.49-.36 2.18" />
      <line x1="12" y1="19" x2="12" y2="23" />
      <line x1="8" y1="23" x2="16" y2="23" />
    </svg>
  );
}

function DeafenedIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="1" y1="1" x2="23" y2="23" />
      <path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3" />
    </svg>
  );
}

function PriorityIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
    </svg>
  );
}

// -- Component -----------------------------------------------------

interface UserListItemProps {
  readonly user: UserEntry;
  /** Channel name shown as a chip (e.g. in the "Online" list). */
  readonly channelName?: string;
  /** Whether this item is currently active/selected. */
  readonly active?: boolean;
  /** Whether this item represents the current user. */
  readonly isSelf?: boolean;
  /** Called on left click. */
  readonly onClick?: () => void;
  /** Called on right click to open context menu. */
  readonly onContextMenu?: (e: React.MouseEvent) => void;
}

export function UserListItem({
  user,
  channelName,
  active,
  isSelf,
  onClick,
  onContextMenu,
}: UserListItemProps) {
  const [showCard, setShowCard] = useState(false);
  const [cardPos, setCardPos] = useState<{ top: number; left: number } | null>(null);
  const itemRef = useRef<HTMLButtonElement>(null);
  const dmUnread = useAppStore((s) => s.dmUnreadCounts[user.session] ?? 0);
  const stats = useUserStats(user.session, showCard);

  const url = useMemo(() => avatarUrl(user), [user.texture]);
  const parsed = useMemo(
    () => (user.comment ? parseComment(user.comment) : null),
    [user.comment],
  );

  const isMuted = user.mute || user.self_mute;
  const isDeafened = user.deaf || user.self_deaf;
  const isPriority = user.priority_speaker;
  const isMobile = isMobilePlatform();

  const handleEnter = useCallback(() => {
    if (isMobile) return;
    if (itemRef.current) {
      const rect = itemRef.current.getBoundingClientRect();
      const rawTop = rect.top + rect.height / 2;
      const top = Math.max(
        HOVER_CARD_H / 2 + HOVER_CARD_MARGIN,
        Math.min(rawTop, window.innerHeight - HOVER_CARD_H / 2 - HOVER_CARD_MARGIN),
      );
      const fitsRight = rect.right + HOVER_CARD_GAP + HOVER_CARD_W + HOVER_CARD_MARGIN <= window.innerWidth;
      const left = fitsRight
        ? rect.right + HOVER_CARD_GAP
        : rect.left - HOVER_CARD_GAP - HOVER_CARD_W;
      setCardPos({ top, left });
    }
    setShowCard(true);
  }, [isMobile]);

  const handleLeave = useCallback(() => {
    setShowCard(false);
  }, []);

  return (
    <button
      ref={itemRef}
      type="button"
      className={`${styles.userItem} ${active ? styles.userItemActive : ""} ${isSelf ? styles.selfUser : ""}`}
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
      onClick={onClick}
      onContextMenu={onContextMenu}
    >
      <div className={styles.avatarWrap}>
        {url ? (
          <img src={url} alt={user.name} className={styles.avatarImg} />
        ) : (
          <div
            className={styles.avatar}
            style={{ background: colorFor(user.name) }}
          >
            {user.name.charAt(0).toUpperCase()}
          </div>
        )}
        <span className={styles.onlineDot} />
      </div>
      <span className={styles.userName}>{user.name}</span>
      {!isSelf && (isMuted || isDeafened || isPriority) && (
        <span className={styles.statusIcons}>
          {isMuted && !isDeafened && (
            <span className={`${styles.statusIcon} ${styles.muted}`} title={user.mute ? "Server muted" : "Self muted"}>
              <MutedIcon />
            </span>
          )}
          {isDeafened && (
            <span className={`${styles.statusIcon} ${styles.deafened}`} title={user.deaf ? "Server deafened" : "Self deafened"}>
              <DeafenedIcon />
            </span>
          )}
          {isPriority && (
            <span className={`${styles.statusIcon} ${styles.prioritySpeaker}`} title="Priority speaker">
              <PriorityIcon />
            </span>
          )}
        </span>
      )}
      {dmUnread > 0 && (
        <span className={styles.unreadBadge}>
          {dmUnread > 99 ? "99+" : dmUnread}
        </span>
      )}
      {channelName && <span className={styles.channelChip}>{channelName}</span>}
      {showCard && cardPos && createPortal(
        <div
          className={styles.profilePopover}
          style={{ top: cardPos.top, left: cardPos.left }}
        >
          <ProfilePreviewCard
            profile={parsed?.profile ?? {}}
            bio={parsed?.bio ?? ""}
            avatar={url}
            displayName={user.name}
            onlinesecs={stats?.onlinesecs}
            idlesecs={stats?.idlesecs}
          />
        </div>,
        document.body,
      )}
    </button>
  );
}
