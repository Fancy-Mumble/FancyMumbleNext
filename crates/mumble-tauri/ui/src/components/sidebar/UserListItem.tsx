import { memo, useState, useMemo, useCallback, useRef } from "react";
import { createPortal } from "react-dom";
import { useAppStore } from "../../store";
import type { UserEntry, FancyProfile } from "../../types";
import { textureToDataUrl, parseComment } from "../../profileFormat";
import { ProfilePreviewCard } from "../../pages/settings/ProfilePreviewCard";
import { useUserStats } from "../../hooks/useUserStats";
import { colorFor } from "../../utils/format";
import { isMobile } from "../../utils/platform";
import MicOffIcon from "../../assets/icons/audio/mic-off.svg?react";
import HeadphonesOffIcon from "../../assets/icons/audio/headphones-off.svg?react";
import StarIcon from "../../assets/icons/status/star.svg?react";
import ShieldCheckIcon from "../../assets/icons/status/shield-check.svg?react";
import VolumeIcon from "../../assets/icons/audio/volume.svg?react";
import ScreenShareIcon from "../../assets/icons/communication/screen-share.svg?react";
import { useStreamThumbnail } from "../chat/useStreamPreview";
import styles from "./UserListItem.module.css";

// Re-export so existing consumers (e.g. ChannelSidebar) keep working.
export { colorFor } from "../../utils/format";

const textureCache = new Map<number, { len: number; url: string }>();

export function avatarUrl(user: UserEntry): string | null {
  if (!user.texture || user.texture.length === 0) return null;
  const cached = textureCache.get(user.session);
  if (cached?.len === user.texture.length) return cached.url;
  const url = textureToDataUrl(user.texture);
  textureCache.set(user.session, { len: user.texture.length, url });
  return url;
}

// -- Shared hover card constants ----------------------------------

const HOVER_CARD_W = 260;
const HOVER_CARD_H = 340;
const HOVER_CARD_MARGIN = 10;
const HOVER_CARD_GAP = 8;

// -- Shared hover card hook ---------------------------------------

interface HoverCardPosition {
  showCard: boolean;
  cardPos: { top: number; left: number } | null;
  itemRef: React.RefObject<HTMLButtonElement | null>;
  handleEnter: () => void;
  handleLeave: () => void;
}

/** Computes the hover card position relative to the hovered element. */
export function useHoverCardPosition(isBroadcasting: boolean): HoverCardPosition {
  const [showCard, setShowCard] = useState(false);
  const [cardPos, setCardPos] = useState<{ top: number; left: number } | null>(null);
  const itemRef = useRef<HTMLButtonElement>(null);

  const handleEnter = useCallback(() => {
    if (isMobile) return;
    if (itemRef.current) {
      const rect = itemRef.current.getBoundingClientRect();
      const rawTop = rect.top + rect.height / 2;
      const effectiveH = isBroadcasting ? HOVER_CARD_H + 160 : HOVER_CARD_H;
      const top = Math.max(
        effectiveH / 2 + HOVER_CARD_MARGIN,
        Math.min(rawTop, window.innerHeight - effectiveH / 2 - HOVER_CARD_MARGIN),
      );
      const fitsRight = rect.right + HOVER_CARD_GAP + HOVER_CARD_W + HOVER_CARD_MARGIN <= window.innerWidth;
      const left = fitsRight
        ? rect.right + HOVER_CARD_GAP
        : rect.left - HOVER_CARD_GAP - HOVER_CARD_W;
      setCardPos({ top, left });
    }
    setShowCard(true);
  }, [isBroadcasting]);

  const handleLeave = useCallback(() => {
    setShowCard(false);
  }, []);

  return { showCard, cardPos, itemRef, handleEnter, handleLeave };
}

// -- Shared hover card portal component ---------------------------

interface UserHoverCardPortalProps {
  readonly displayName: string;
  readonly cardPos: { top: number; left: number };
  readonly avatar: string | null;
  readonly profile: FancyProfile;
  readonly bio: string;
  readonly onlinesecs?: number | null;
  readonly idlesecs?: number | null;
  readonly isRegistered: boolean;
  readonly isBroadcasting: boolean;
  readonly thumbnail: string | null;
}

/** Portal overlay that renders the profile card + optional stream preview thumbnail. */
export function UserHoverCardPortal({
  displayName,
  cardPos,
  avatar,
  profile,
  bio,
  onlinesecs,
  idlesecs,
  isRegistered,
  isBroadcasting,
  thumbnail,
}: Readonly<UserHoverCardPortalProps>) {
  return createPortal(
    <div
      className={styles.profilePopover}
      style={{ top: cardPos.top, left: cardPos.left }}
    >
      {isBroadcasting && (
        <div className={styles.streamPreview}>
          {thumbnail ? (
            <img src={thumbnail} alt="Screen share preview" />
          ) : (
            <div className={styles.streamPreviewPlaceholder}>
              <ScreenShareIcon width={24} height={24} />
            </div>
          )}
        </div>
      )}
      <ProfilePreviewCard
        profile={profile}
        bio={bio}
        avatar={avatar}
        displayName={displayName}
        onlinesecs={onlinesecs}
        idlesecs={idlesecs}
        isRegistered={isRegistered}
      />
    </div>,
    document.body,
  );
}

// -- SVG icons -----------------------------------------------------

function MutedIcon() {
  return <MicOffIcon width={14} height={14} />;
}

function DeafenedIcon() {
  return <HeadphonesOffIcon width={14} height={14} />;
}

function PriorityIcon() {
  return <StarIcon width={14} height={14} />;
}

function RegisteredIcon() {
  return <ShieldCheckIcon width={14} height={14} />;
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
  /** Whether this user is currently transmitting audio (talking). */
  readonly isTalking?: boolean;
  /** Called on left click. */
  readonly onClick?: () => void;
  /** Called on right click to open context menu. */
  readonly onContextMenu?: (e: React.MouseEvent) => void;
}

export const UserListItem = memo(function UserListItem({
  user,
  channelName,
  active,
  isSelf,
  isTalking,
  onClick,
  onContextMenu,
}: UserListItemProps) {
  const dmUnread = useAppStore((s) => s.dmUnreadCounts[user.session] ?? 0);
  const volumePct = useAppStore((s) => user.hash ? (s.userVolumes[user.hash] ?? 100) : 100);
  const isBroadcasting = useAppStore((s) => s.broadcastingSessions.has(user.session));
  const { showCard, cardPos, itemRef, handleEnter, handleLeave } = useHoverCardPosition(isBroadcasting);
  const stats = useUserStats(user.session, showCard);
  const streamThumbnail = useStreamThumbnail(user.session, showCard && isBroadcasting);

  const url = useMemo(() => avatarUrl(user), [user.texture]);
  const parsed = useMemo(
    () => (user.comment ? parseComment(user.comment) : null),
    [user.comment],
  );

  const isMuted = user.mute || user.self_mute;
  const isDeafened = user.deaf || user.self_deaf;
  const isPriority = user.priority_speaker;
  const isRegistered = user.user_id != null && user.user_id > 0;

  return (
    <button
      ref={itemRef}
      type="button"
      className={`${styles.userItem} ${active ? styles.userItemActive : ""} ${isSelf ? styles.selfUser : ""} ${isSelf && isTalking ? styles.selfTalking : ""}`}
      data-clickable={isSelf && onClick ? "true" : undefined}
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
      onClick={onClick}
      onContextMenu={onContextMenu}
    >
      <div className={`${styles.avatarWrap} ${isTalking ? styles.avatarTalking : ""}`}>
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
      {!isSelf && volumePct !== 100 && (
        <span className={styles.volumeBadge} title={`Volume: ${volumePct}%`}>
          <VolumeIcon width={12} height={12} />
          {volumePct}%
        </span>
      )}
      {(isRegistered || (!isSelf && (isMuted || isDeafened || isPriority))) && (
        <span className={styles.statusIcons}>
          {isRegistered && (
            <span className={`${styles.statusIcon} ${styles.registered}`} title="Registered">
              <RegisteredIcon />
            </span>
          )}
          {!isSelf && isMuted && !isDeafened && (
            <span className={`${styles.statusIcon} ${styles.muted}`} title={user.mute ? "Server muted" : "Self muted"}>
              <MutedIcon />
            </span>
          )}
          {!isSelf && isDeafened && (
            <span className={`${styles.statusIcon} ${styles.deafened}`} title={user.deaf ? "Server deafened" : "Self deafened"}>
              <DeafenedIcon />
            </span>
          )}
          {!isSelf && isPriority && (
            <span className={`${styles.statusIcon} ${styles.prioritySpeaker}`} title="Priority speaker">
              <PriorityIcon />
            </span>
          )}
        </span>
      )}
      {isBroadcasting && (
        <span className={styles.liveBadge} title="Sharing screen">
          <ScreenShareIcon width={10} height={10} />
          Live
        </span>
      )}
      {dmUnread > 0 && (
        <span className={styles.unreadBadge}>
          {dmUnread > 99 ? "99+" : dmUnread}
        </span>
      )}
      {channelName && <span className={styles.channelChip}>{channelName}</span>}
      {showCard && cardPos && (
        <UserHoverCardPortal
          displayName={user.name}
          cardPos={cardPos}
          avatar={url}
          profile={parsed?.profile ?? {}}
          bio={parsed?.bio ?? ""}
          onlinesecs={stats?.onlinesecs}
          idlesecs={stats?.idlesecs}
          isRegistered={isRegistered}
          isBroadcasting={isBroadcasting}
          thumbnail={streamThumbnail}
        />
      )}
    </button>
  );
});
