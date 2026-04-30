import { HashIcon, HeadphonesOffIcon, ListenBadgeIcon, MicOffSmallIcon, ScreenShareIcon } from "../../../icons";
/**
 * ChannelIconList - a "Modern" channel viewer.
 *
 * - Flat, no hierarchy.
 * - Round channel icon on the left: first <img> from description, or initials fallback.
 * - Channel name and member count on the right.
 * - Inline member avatars below on expand.
 * - Populated channels sorted to the top.
 * - Current channel sticky at the top with accent left border.
 */

import { useState, useMemo, useCallback, useContext } from "react";
import type { ChannelEntry, UserEntry } from "../../../types";
import { colorFor, useHoverCardPosition, UserHoverCardPortal, RoleColorsContext } from "../UserListItem";
import { useUserAvatar, useChannelDescription } from "../../../lazyBlobs";
import { parseComment } from "../../../profileFormat";
import { useUserStats } from "../../../hooks/useUserStats";
import { useStreamThumbnail } from "../../chat/useStreamPreview";
import SwipeableCard from "../../elements/SwipeableCard";
import { isMobile } from "../../../utils/platform";
import { PchatBadge } from "../PchatBadge";
import styles from "./ChannelIconList.module.css";

/** Extract the src of the first <img> tag in an HTML string. */
function extractDescriptionImage(html: string): string | null {
  const match = /<img[^>]+src=["']([^"']+)["']/i.exec(html);
  return match ? match[1] : null;
}

export interface ChannelIconListProps {
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

// -- Channel icon (description image or initials fallback) ---------

interface ChannelIconProps {
  readonly channel: ChannelEntry;
  readonly isCurrent: boolean;
}

function ChannelIcon({ channel, isCurrent }: ChannelIconProps) {
  const description = useChannelDescription(channel.id, channel.description_size);
  const imgSrc = useMemo(
    () => (description ? extractDescriptionImage(description) : null),
    [description],
  );

  if (imgSrc) {
    return (
      <div className={`${styles.channelIcon} ${isCurrent ? styles.channelIconCurrent : ""}`}>
        <img src={imgSrc} alt="" className={styles.channelIconImg} />
      </div>
    );
  }

  const initial = (channel.name || "#").charAt(0).toUpperCase();
  const color = colorFor(channel.name);

  return (
    <div
      className={`${styles.channelIcon} ${styles.channelIconFallback} ${isCurrent ? styles.channelIconCurrent : ""}`}
      style={{ background: color }}
    >
      {initial === "#" ? (
        <HashIcon width={16} height={16} className={styles.channelIconHash} />
      ) : (
        <span className={styles.channelIconInitial}>{initial}</span>
      )}
    </div>
  );
}

// -- Member row inside expanded channel ---------------------------

interface MemberRowProps {
  readonly user: UserEntry;
  readonly isTalking: boolean;
  readonly isBroadcasting: boolean;
  readonly onContextMenu?: (e: React.MouseEvent, user: UserEntry) => void;
  readonly onClick?: (session: number) => void;
}

function MemberRow({ user, isTalking, isBroadcasting, onContextMenu, onClick }: MemberRowProps) {
  const roleColors = useContext(RoleColorsContext);
  const roleColor = user.user_id != null ? (roleColors.get(user.user_id) ?? null) : null;
  const url = useUserAvatar(user.session, user.texture_size);
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
        className={`${styles.memberRow} ${isTalking ? styles.memberTalking : ""}`}
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

// -- Main component ------------------------------------------------

export default function ChannelIconList({
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
}: ChannelIconListProps) {
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());

  const toggleCollapsed = useCallback((id: number) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const usersByChannel = useMemo(() => {
    const map = new Map<number, UserEntry[]>();
    for (const u of users) {
      const list = map.get(u.channel_id) ?? [];
      list.push(u);
      map.set(u.channel_id, list);
    }
    return map;
  }, [users]);

  const flatChannels = useMemo(() => {
    const root = channels.find((c) => c.parent_id === null || c.parent_id === c.id);
    const rootId = root?.id ?? 0;
    const all = channels.filter((c) => c.id !== rootId);
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

  const currentEntry = useMemo(
    () => (currentChannel == null ? undefined : flatChannels.find((c) => c.id === currentChannel)),
    [flatChannels, currentChannel],
  );

  const otherChannels = useMemo(
    () => (currentChannel == null ? flatChannels : flatChannels.filter((c) => c.id !== currentChannel)),
    [flatChannels, currentChannel],
  );

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
        className={[
          styles.channelRow,
          isSelected ? styles.selected : "",
          isCurrent ? styles.current : "",
        ].filter(Boolean).join(" ")}
      >
        <div className={styles.channelMain}>
          <ChannelIcon channel={channel} isCurrent={isCurrent} />

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
                  <ListenBadgeIcon width={11} height={11} />
                </span>
              )}
              <PchatBadge protocol={channel.pchat_protocol} />
            </span>
          </button>

          <div className={styles.channelMeta}>
            {unread > 0 && (
              <span className={styles.unreadBadge}>
                {unread > 99 ? "99+" : unread}
              </span>
            )}
            {hasUsers && (
              <button
                type="button"
                className={styles.memberCountBtn}
                onClick={() => toggleCollapsed(channel.id)}
                title={isCollapsed ? "Show members" : "Hide members"}
              >
                {chUsers.length}
              </button>
            )}
          </div>
        </div>

        {!isCollapsed && hasUsers && (
          <div className={styles.memberList}>
            {chUsers.map((u) => (
              <MemberRow
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
      {currentEntry && (
        <div className={styles.stickyChannel}>
          {renderChannel(currentEntry)}
        </div>
      )}

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
