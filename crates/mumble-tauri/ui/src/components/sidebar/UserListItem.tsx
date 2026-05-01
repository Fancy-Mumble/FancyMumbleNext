import { HeadphonesOffIcon, MicOffIcon, ScreenShareIcon, ShieldCheckIcon, StarIcon, VolumeIcon } from "../../icons";
import { memo, useState, useMemo, useCallback, useEffect, useRef, createContext, useContext } from "react";
import { createPortal } from "react-dom";
import { useAppStore } from "../../store";
import type { UserEntry, FancyProfile, AclGroup } from "../../types";
import { parseComment } from "../../profileFormat";
import { useUserAvatar } from "../../lazyBlobs";
import { ProfilePreviewCard } from "../../pages/settings/ProfilePreviewCard";
import { useUserStats } from "../../hooks/useUserStats";
import { colorFor } from "../../utils/format";
import { isMobile } from "../../utils/platform";
import { PERM_MOVE as PERM_MOVE_BIT } from "../../utils/permissions";
import { useUserDrag } from "../../utils/userMoveDnd";
import { useStreamThumbnail } from "../chat/useStreamPreview";
import styles from "./UserListItem.module.css";

// Re-export so existing consumers (e.g. ChannelSidebar) keep working.
export { colorFor } from "../../utils/format";

// -- Role colour context -----------------------------------------

/** Reject any value that could escape the inline style attribute. */
function sanitiseRoleColor(raw: string): string | null {
  const v = raw.trim();
  if (v.length === 0 || v.length > 64) return null;
  if (!/^[#A-Za-z0-9 .,()%/]+$/.test(v)) return null;
  return v;
}

/**
 * Build a `user_id -> CSS color` map from the ACL group list.
 *
 * For each registered user the first group (in array order) that
 * - contains them in `add` or `inherited_members` (and not in `remove`)
 * - has a non-null `color` property
 * - is not a system group (name not starting with `~`)
 * determines their display color.
 */
export function buildRoleColorMap(
  groups: readonly AclGroup[],
): ReadonlyMap<number, string> {
  const map = new Map<number, string>();
  for (const g of groups) {
    if (g.name.startsWith("~")) continue;
    const raw = g.color;
    if (!raw) continue;
    const color = sanitiseRoleColor(raw);
    if (!color) continue;
    const removeSet = new Set(g.remove);
    const members = [...g.add, ...g.inherited_members];
    for (const uid of members) {
      if (!removeSet.has(uid) && !map.has(uid)) {
        map.set(uid, color);
      }
    }
  }
  return map;
}

/** A single role chip: the group name and its optional CSS color. */
export type RoleChip = { readonly name: string; readonly color: string | null };

/**
 * Build a `user_id -> [RoleChip]` map from the ACL group list.
 * Every non-system group (name not starting with `~`) that contains a user
 * contributes a chip for that user.
 */
export function buildRoleGroupsMap(
  groups: readonly AclGroup[],
): ReadonlyMap<number, readonly RoleChip[]> {
  const map = new Map<number, RoleChip[]>();
  for (const g of groups) {
    if (g.name.startsWith("~")) continue;
    const color = g.color ? sanitiseRoleColor(g.color) : null;
    const chip: RoleChip = { name: g.name, color };
    const removeSet = new Set(g.remove);
    for (const uid of [...g.add, ...g.inherited_members]) {
      if (removeSet.has(uid)) continue;
      const list = map.get(uid);
      if (list) list.push(chip);
      else map.set(uid, [chip]);
    }
  }
  return map;
}

/**
 * React context that provides a `user_id -> CSS color` map.
 * Provided once at the ChannelSidebar level; consumed by any component
 * that renders a user display name.
 */
export const RoleColorsContext = createContext<ReadonlyMap<number, string>>(new Map());

/** Provides `user_id -> RoleChip[]` to every `UserListItem` in the tree. */
export const RoleGroupsContext = createContext<ReadonlyMap<number, readonly RoleChip[]>>(new Map());

// LRU avatar caching now lives in `lazyBlobs.ts` (`useUserAvatar`).  Avatars
// are fetched lazily over IPC because the bulk `get_users` payload only
// includes the texture byte length, not the bytes themselves.

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
  /** ACL groups this user belongs to, shown as chips on the card. */
  readonly groups?: readonly RoleChip[];
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
  groups,
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
        groups={groups}
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
  /** Whether this user is offline (registered but not connected). */
  readonly offline?: boolean;
  /** Called on left click. */
  readonly onClick?: () => void;
  /** Called on right click to open context menu. */
  readonly onContextMenu?: (e: React.MouseEvent) => void;
  /**
   * Called when the hover card opens and the user has no comment yet.
   * The parent is responsible for deduplication and sending the blob request.
   */
  readonly onRequestComment?: (userId: number) => void;
}

export const UserListItem = memo(function UserListItem({
  user,
  channelName,
  active,
  isSelf,
  isTalking,
  offline,
  onClick,
  onContextMenu,
  onRequestComment,
}: UserListItemProps) {
  const roleColors = useContext(RoleColorsContext);
  const roleColor = user.user_id != null ? (roleColors.get(user.user_id) ?? null) : null;
  const roleGroups = useContext(RoleGroupsContext);
  const userGroups = user.user_id != null ? (roleGroups.get(user.user_id) ?? []) : [];
  const dmUnread = useAppStore((s) => s.dmUnreadCounts[user.session] ?? 0);
  const volumePct = useAppStore((s) => user.hash ? (s.userVolumes[user.hash] ?? 100) : 100);
  const isBroadcasting = useAppStore((s) => s.broadcastingSessions.has(user.session));
  const canMoveUser = useAppStore((s) => {
    const ch = s.channels.find((c) => c.id === user.channel_id);
    return ch?.permissions != null && (ch.permissions & PERM_MOVE_BIT) !== 0;
  });
  const { showCard, cardPos, itemRef, handleEnter, handleLeave } = useHoverCardPosition(isBroadcasting);
  const stats = useUserStats(user.session, showCard);
  const streamThumbnail = useStreamThumbnail(user.session, showCard && isBroadcasting);

  const url = useUserAvatar(user.session, user.texture_size);
  const parsed = useMemo(
    () => (user.comment ? parseComment(user.comment) : null),
    [user.comment],
  );

  const isMuted = user.mute || user.self_mute;
  const isDeafened = user.deaf || user.self_deaf;
  const isPriority = user.priority_speaker;
  const isRegistered = user.user_id != null && user.user_id > 0;

  useEffect(() => {
    if (showCard && offline && !user.comment && user.user_id != null) {
      onRequestComment?.(user.user_id);
    }
  }, [showCard, offline, user.comment, user.user_id, onRequestComment]);

  const dragDisabled = isMobile || isSelf || !!offline || !canMoveUser;
  const { handlers: dragHandlers, overlay: dragOverlay } = useUserDrag(
    user.session,
    user.name,
    url,
    dragDisabled,
  );

  return (
    <>
    {dragOverlay}
    <button
      ref={itemRef}
      type="button"
      className={`${styles.userItem} ${active ? styles.userItemActive : ""} ${isSelf ? styles.selfUser : ""} ${isSelf && isTalking ? styles.selfTalking : ""} ${offline ? styles.userItemOffline : ""}`}
      data-clickable={isSelf && onClick ? "true" : undefined}
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
      onClick={onClick}
      onClickCapture={dragHandlers.onClickCapture}
      onContextMenu={onContextMenu}
      onPointerDown={dragHandlers.onPointerDown}
      onPointerMove={dragHandlers.onPointerMove}
      onPointerUp={dragHandlers.onPointerUp}
      onPointerCancel={dragHandlers.onPointerCancel}
      style={dragHandlers.style}
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
        {!offline && <span className={styles.onlineDot} />}
      </div>
      <span
        className={styles.userName}
        style={roleColor ? { color: roleColor } : undefined}
      >{user.name}</span>
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
          groups={userGroups.length > 0 ? userGroups : undefined}
        />
      )}
    </button>
    </>
  );
});
