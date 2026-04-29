import { ChevronRightIcon, HeadphonesOffIcon, ListenBadgeIcon, MicOffSmallIcon, ScreenShareIcon } from "../../../icons";
/**
 * ModernChannelList - a flat, always-visible channel viewer.
 *
 * - No hierarchy: all channels rendered at the same level.
 * - Channels with members are sorted to the top.
 * - Each channel shows its members directly below the name.
 * - Channels can be collapsed (shows stacked avatar bubbles instead).
 * - Default state: expanded (members visible as a name list).
 * - Current channel is sticky at the top when scrolling.
 * - Hovering a member shows their profile card.
 * - Right-clicking a member opens the user context menu.
 */

import { useState, useMemo, useCallback, useContext } from "react";
import type { ChannelEntry, UserEntry } from "../../../types";
import { colorFor, avatarUrl, useHoverCardPosition, UserHoverCardPortal, RoleColorsContext } from "../UserListItem";
import { parseComment } from "../../../profileFormat";
import { useUserStats } from "../../../hooks/useUserStats";
import { useStreamThumbnail } from "../../chat/useStreamPreview";
import SwipeableCard from "../../elements/SwipeableCard";
import { isMobile } from "../../../utils/platform";
import { PchatBadge } from "../PchatBadge";
import styles from "./ModernChannelList.module.css";

const MAX_STACKED = 3;

interface ModernChannelListProps {
  readonly channels: ChannelEntry[];
  readonly users: UserEntry[];
  readonly selectedChannel: number | null;
  readonly currentChannel: number | null;
  readonly listenedChannels: Set<number>;
  readonly unreadCounts: Record<number, number>;
  readonly talkingSessions: Set<number>;
  readonly broadcastingSessions: Set<number>;
  readonly onSelectChannel: (id: number) => void;
  readonly onJoinChannel: (id: number) => void;
  readonly onContextMenu: (e: React.MouseEvent, channelId: number) => void;
  readonly onUserContextMenu?: (e: React.MouseEvent, user: UserEntry) => void;
  readonly onUserClick?: (session: number) => void;
}

// -- Member item with hover card ----------------------------------

interface MemberItemProps {
  readonly user: UserEntry;
  readonly isTalking: boolean;
  readonly isBroadcasting: boolean;
  readonly onContextMenu?: (e: React.MouseEvent, user: UserEntry) => void;
  readonly onClick?: (session: number) => void;
}

function MemberItem({ user, isTalking, isBroadcasting, onContextMenu, onClick }: MemberItemProps) {
  const roleColors = useContext(RoleColorsContext);
  const roleColor = user.user_id != null ? (roleColors.get(user.user_id) ?? null) : null;
  const url = useMemo(() => avatarUrl(user), [user.texture]);
  const parsed = useMemo(
    () => (user.comment ? parseComment(user.comment) : null),
    [user.comment],
  );
  const { showCard, cardPos, itemRef, handleEnter, handleLeave } = useHoverCardPosition(isBroadcasting);
  const stats = useUserStats(user.session, showCard);
  const streamThumbnail = useStreamThumbnail(user.session, showCard && isBroadcasting);

  const handleContextMenu = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      onContextMenu?.(e, user);
    },
    [onContextMenu, user],
  );

  return (
    <>
      <button
        ref={itemRef}
        type="button"
        className={`${styles.memberItem} ${isTalking ? styles.memberTalking : ""}`}
        onMouseEnter={handleEnter}
        onMouseLeave={handleLeave}
        onContextMenu={handleContextMenu}
        onClick={() => onClick?.(user.session)}
      >
        <div
          className={styles.memberAvatar}
          style={{ background: url ? "transparent" : colorFor(user.name) }}
        >
          {url ? (
            <img src={url} alt={user.name} className={styles.memberAvatarImg} />
          ) : (
            user.name.charAt(0).toUpperCase()
          )}
        </div>
        <span
          className={styles.memberName}
          style={roleColor ? { color: roleColor } : undefined}
        >{user.name}</span>
        {user.self_mute && (
          <MicOffSmallIcon className={styles.statusIcon} width={12} height={12} />
        )}
        {user.self_deaf && (
          <HeadphonesOffIcon className={styles.statusIcon} width={12} height={12} />
        )}
        {isBroadcasting && (
          <span className={styles.liveBadge} title="Sharing screen">
            <ScreenShareIcon width={10} height={10} />
            Live
          </span>
        )}
      </button>
      {showCard && cardPos && (
        <UserHoverCardPortal
          displayName={user.name}
          cardPos={cardPos}
          avatar={url}
          profile={parsed?.profile ?? {}}
          bio={parsed?.bio ?? ""}
          onlinesecs={stats?.onlinesecs}
          idlesecs={stats?.idlesecs}
          isRegistered={user.user_id != null && user.user_id > 0}
          isBroadcasting={isBroadcasting}
          thumbnail={streamThumbnail}
        />
      )}
    </>
  );
}

/** Small inline avatars shown when a channel is collapsed. */
function CollapsedAvatars({ users }: Readonly<{ users: UserEntry[] }>) {
  if (users.length === 0) return null;
  const visible = users.slice(0, MAX_STACKED);
  const overflow = users.length - MAX_STACKED;

  return (
    <div className={styles.collapsedAvatars}>
      {visible.map((u) => {
        const url = avatarUrl(u);
        return (
          <div
            key={u.session}
            className={styles.collapsedAvatar}
            style={{ background: url ? "transparent" : colorFor(u.name) }}
            title={u.name}
          >
            {url ? (
              <img src={url} alt={u.name} className={styles.collapsedAvatarImg} />
            ) : (
              u.name.charAt(0).toUpperCase()
            )}
          </div>
        );
      })}
      {overflow > 0 && (
        <span className={styles.overflowCount}>+{overflow}</span>
      )}
    </div>
  );
}

export default function ModernChannelList({
  channels,
  users,
  selectedChannel,
  currentChannel,
  listenedChannels,
  unreadCounts,
  talkingSessions,
  broadcastingSessions,
  onSelectChannel,
  onJoinChannel,
  onContextMenu,
  onUserContextMenu,
  onUserClick,
}: ModernChannelListProps) {
  // Collapsed channels (expanded by default = not in the set).
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());

  const toggleCollapsed = useCallback((id: number) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  // Build a map of users per channel.
  const usersByChannel = useMemo(() => {
    const map = new Map<number, UserEntry[]>();
    for (const u of users) {
      const list = map.get(u.channel_id) ?? [];
      list.push(u);
      map.set(u.channel_id, list);
    }
    return map;
  }, [users]);

  // Flat list of all channels, excluding the root itself.
  // Sorted: channels with members first, then alphabetical.
  const flatChannels = useMemo(() => {
    const root = channels.find(
      (c) => c.parent_id === null || c.parent_id === c.id,
    );
    const rootId = root?.id ?? 0;

    // Include all channels (root + descendants).
    const all = channels.filter((c) => c.id !== rootId);
    // Also include root if it has users.
    if (root && (usersByChannel.get(root.id)?.length ?? 0) > 0) {
      all.unshift(root);
    }

    return all.sort((a, b) => {
      const aUsers = usersByChannel.get(a.id)?.length ?? 0;
      const bUsers = usersByChannel.get(b.id)?.length ?? 0;
      if (aUsers > 0 && bUsers === 0) return -1;
      if (aUsers === 0 && bUsers > 0) return 1;
      return a.name.localeCompare(b.name);
    });
  }, [channels, usersByChannel]);

  // Find the current channel entry for the sticky header.
  const currentChannelEntry = useMemo(
    () => (currentChannel == null ? undefined : flatChannels.find((c) => c.id === currentChannel)),
    [flatChannels, currentChannel],
  );

  // Channels excluding the current one (rendered below the sticky header).
  const otherChannels = useMemo(
    () => (currentChannel == null ? flatChannels : flatChannels.filter((c) => c.id !== currentChannel)),
    [flatChannels, currentChannel],
  );

  /** Render a single channel card (shared between sticky and list). */
  const renderChannel = useCallback((channel: ChannelEntry) => {
    const chUsers = usersByChannel.get(channel.id) ?? [];
    const unread = unreadCounts[channel.id] ?? 0;
    const isListened = listenedChannels.has(channel.id);
    const isSelected = selectedChannel === channel.id;
    const isCurrent = currentChannel === channel.id;
    const isCollapsed = collapsed.has(channel.id);
    const hasUsers = chUsers.length > 0;

    return (
      <div
        className={`${styles.channelCard} ${isSelected ? styles.selected : ""} ${isCurrent ? styles.current : ""}`}
      >
        {/* Channel header row */}
        <div className={styles.headerRow}>
          {hasUsers && (
            <button
              type="button"
              className={styles.expandBtn}
              onClick={() => toggleCollapsed(channel.id)}
              aria-label={isCollapsed ? "Expand" : "Collapse"}
            >
              <ChevronRightIcon
                className={`${styles.chevron} ${isCollapsed ? "" : styles.chevronOpen}`}
                width={12}
                height={12}
              />
            </button>
          )}

          <button
            type="button"
            className={styles.channelBtn}
            onClick={() => onSelectChannel(channel.id)}
            onDoubleClick={() => onJoinChannel(channel.id)}
            onContextMenu={(e) => onContextMenu(e, channel.id)}
          >
            <span className={styles.channelName}>
              {channel.name || "Root"}
              {isListened && (
                <span className={styles.listenBadge} title="Listening">
                  <ListenBadgeIcon width={12} height={12} />
                </span>
              )}
              <PchatBadge protocol={channel.pchat_protocol} />
            </span>
            {hasUsers && (
              <span className={styles.memberCount}>
                {chUsers.length}
              </span>
            )}
          </button>

          {unread > 0 && (
            <span className={styles.unreadBadge}>
              {unread > 99 ? "99+" : unread}
            </span>
          )}

          {/* Collapsed: show stacked avatar bubbles */}
          {isCollapsed && hasUsers && (
            <CollapsedAvatars users={chUsers} />
          )}
        </div>

        {/* Expanded: show member names */}
        {!isCollapsed && hasUsers && (
          <div className={styles.memberList}>
            {chUsers.map((u) => (
              <MemberItem
                key={u.session}
                user={u}
                isTalking={talkingSessions.has(u.session)}
                isBroadcasting={broadcastingSessions.has(u.session)}
                onContextMenu={onUserContextMenu}
                onClick={onUserClick}
              />
            ))}
          </div>
        )}
      </div>
    );
  }, [
    usersByChannel, unreadCounts, listenedChannels, selectedChannel,
    currentChannel, collapsed, talkingSessions, broadcastingSessions,
    toggleCollapsed, onSelectChannel, onJoinChannel, onContextMenu, onUserContextMenu, onUserClick,
  ]);

  return (
    <div className={styles.list}>
      {/* Sticky current channel */}
      {currentChannelEntry && (
        <div className={styles.stickyChannel}>
          {renderChannel(currentChannelEntry)}
        </div>
      )}

      {/* Other channels */}
      {otherChannels.map((channel) => {
        const card = renderChannel(channel);

        if (isMobile) {
          return (
            <SwipeableCard
              key={channel.id}
              rightSwipeAction={{
                label: "Join",
                color: "var(--color-accent, #2aabee)",
                onTrigger: () => onJoinChannel(channel.id),
              }}
            >
              {card}
            </SwipeableCard>
          );
        }

        return <div key={channel.id}>{card}</div>;
      })}
    </div>
  );
}
