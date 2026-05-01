import { ChevronRightIcon, ListenBadgeIcon } from "../../../icons";
/**
 * ClassicChannelList - the traditional Mumble hierarchical channel tree.
 *
 * - Root channel at the top, then sorted folder groups.
 * - Folders expand/collapse with a chevron button.
 * - Populated folders and channels sorted first.
 * - Stacked user avatars shown on each entry.
 * - Current channel sticky at the top.
 */

import { useState, useMemo, useCallback } from "react";
import type { ChannelEntry, UserEntry } from "../../../types";
import { colorFor } from "../UserListItem";
import { useUserAvatar } from "../../../lazyBlobs";
import { PchatBadge } from "../PchatBadge";
import { useChannelDropTarget } from "../../../utils/userMoveDnd";
import styles from "./ClassicChannelList.module.css";

const MAX_STACKED = 3;

export interface ClassicChannelListProps {
  readonly channels: ChannelEntry[];
  readonly users: UserEntry[];
  readonly selectedChannel: number | null;
  readonly currentChannel: number | null;
  readonly listenedChannels: Set<number>;
  readonly unreadCounts: Record<number, number>;
  readonly onSelectChannel: (id: number) => void;
  readonly onJoinChannel: (id: number) => void;
  readonly onContextMenu: (e: React.MouseEvent, channelId: number) => void;
}

// --- Stacked avatars ---------------------------------------------

function StackedAvatar({ user, zIndex }: Readonly<{ user: UserEntry; zIndex: number }>) {
  const url = useUserAvatar(user.session, user.texture_size);
  return (
    <div
      className={styles.stackedAvatar}
      style={{
        background: url ? "transparent" : colorFor(user.name),
        zIndex,
      }}
    >
      {url ? (
        <img src={url} alt={user.name} className={styles.stackedAvatarImg} />
      ) : (
        user.name.charAt(0).toUpperCase()
      )}
    </div>
  );
}

function TooltipUser({ user }: Readonly<{ user: UserEntry }>) {
  const url = useUserAvatar(user.session, user.texture_size);
  return (
    <div className={styles.tooltipUser}>
      {url ? (
        <img src={url} alt={user.name} className={styles.tooltipAvatarImg} />
      ) : (
        <div
          className={styles.tooltipAvatar}
          style={{ background: colorFor(user.name) }}
        >
          {user.name.charAt(0).toUpperCase()}
        </div>
      )}
      <span>{user.name}</span>
    </div>
  );
}

function StackedAvatars({ users }: Readonly<{ users: UserEntry[] }>) {
  const [showTooltip, setShowTooltip] = useState(false);
  if (users.length === 0) return null;

  const visible = users.slice(0, MAX_STACKED);
  const overflow = users.length - MAX_STACKED;

  return (
    <div
      aria-hidden="true"
      className={styles.stackedAvatars}
      onMouseEnter={() => setShowTooltip(true)}
      onMouseLeave={() => setShowTooltip(false)}
    >
      {visible.map((u, i) => (
        <StackedAvatar key={u.session} user={u} zIndex={MAX_STACKED - i} />
      ))}
      {overflow > 0 && (
        <div className={`${styles.stackedAvatar} ${styles.overflowBadge}`}>
          +{overflow}
        </div>
      )}
      {showTooltip && (
        <div className={styles.avatarTooltip}>
          {users.map((u) => (
            <TooltipUser key={u.session} user={u} />
          ))}
        </div>
      )}
    </div>
  );
}

// --- Main component ----------------------------------------------

function ChannelDropWrapper({
  channelId,
  children,
}: Readonly<{ channelId: number; children: React.ReactNode }>) {
  const { ref, active } = useChannelDropTarget(channelId);
  return (
    <div
      ref={ref}
      className={`${styles.dropZone} ${active ? styles.dropZoneActive : ""}`}
    >
      {children}
    </div>
  );
}

export default function ClassicChannelList({
  channels,
  users,
  selectedChannel,
  currentChannel,
  listenedChannels,
  unreadCounts,
  onSelectChannel,
  onJoinChannel,
  onContextMenu,
}: ClassicChannelListProps) {
  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  const toggleExpand = useCallback((id: number) => {
    setExpanded((prev) => {
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

  const root = useMemo(
    () => channels.find((c) => c.parent_id === null || c.parent_id === c.id) ?? null,
    [channels],
  );

  const currentChannelEntry = useMemo(
    () => (currentChannel == null ? null : channels.find((c) => c.id === currentChannel) ?? null),
    [channels, currentChannel],
  );

  function subtreeUserCount(channelId: number): number {
    const direct = usersByChannel.get(channelId)?.length ?? 0;
    const childTotal = channels
      .filter((c) => c.parent_id === channelId && c.id !== channelId)
      .reduce((sum, ch) => sum + subtreeUserCount(ch.id), 0);
    return direct + childTotal;
  }

  function subtreeUsers(channelId: number): UserEntry[] {
    const result: UserEntry[] = [...(usersByChannel.get(channelId) ?? [])];
    for (const ch of channels.filter((c) => c.parent_id === channelId && c.id !== channelId)) {
      result.push(...subtreeUsers(ch.id));
    }
    return result;
  }

  function sortedDirectChildren(parentId: number): ChannelEntry[] {
    return channels
      .filter((c) => c.parent_id === parentId && c.id !== parentId)
      .sort((a, b) => {
        const aUsers = subtreeUserCount(a.id);
        const bUsers = subtreeUserCount(b.id);
        if (aUsers > 0 && bUsers === 0) return -1;
        if (aUsers === 0 && bUsers > 0) return 1;
        return a.name.localeCompare(b.name);
      });
  }

  function renderChannel(channel: ChannelEntry, depth: number) {
    const directChildren = sortedDirectChildren(channel.id);
    const hasChildren = directChildren.length > 0;
    const isOpen = expanded.has(channel.id);
    const chUsers = usersByChannel.get(channel.id) ?? [];
    const unread = unreadCounts[channel.id] ?? 0;
    const isListened = listenedChannels.has(channel.id);
    const isSelected = selectedChannel === channel.id;
    const isCurrent = channel.id === currentChannel;
    const indentPx = depth * 16;

    if (hasChildren) {
      const totalUsers = subtreeUserCount(channel.id);
      const allUsers = subtreeUsers(channel.id);
      return (
        <ChannelDropWrapper key={channel.id} channelId={channel.id}>
        <div className={styles.folderGroup}>
          <div
            className={[
              styles.folderHeader,
              isSelected ? styles.active : "",
              isCurrent ? styles.currentChannel : "",
            ].filter(Boolean).join(" ")}
            style={{ paddingLeft: `${4 + indentPx}px` }}
            role="toolbar"
            onContextMenu={(e) => onContextMenu(e, channel.id)}
          >
            <button
              className={styles.expandBtn}
              onClick={() => toggleExpand(channel.id)}
              aria-label={isOpen ? "Collapse" : "Expand"}
            >
              <ChevronRightIcon
                className={`${styles.chevron} ${isOpen ? styles.chevronOpen : ""}`}
                width={14}
                height={14}
              />
            </button>
            <button
              className={styles.folderSelect}
              onClick={() => onSelectChannel(channel.id)}
              onDoubleClick={() => onJoinChannel(channel.id)}
            >
              <span className={styles.channelName}>
                {channel.name || "Unnamed"}
                {isListened && (
                  <span className={styles.listenIndicator} title="Listening">
                    <ListenBadgeIcon width={12} height={12} />
                  </span>
                )}
                <PchatBadge protocol={channel.pchat_protocol} />
              </span>
              <span className={styles.channelMeta}>
                {totalUsers} {totalUsers === 1 ? "member" : "members"}
              </span>
            </button>
            {unread > 0 && (
              <span className={styles.unreadBadge}>{unread > 99 ? "99+" : unread}</span>
            )}
            <StackedAvatars users={allUsers} />
          </div>
          {isOpen && (
            <div className={styles.folderChildren}>
              {directChildren.map((ch) => renderChannel(ch, depth + 1))}
            </div>
          )}
        </div>
        </ChannelDropWrapper>
      );
    }

    return (
      <ChannelDropWrapper key={channel.id} channelId={channel.id}>
      <button
        className={[
          styles.channelItem,
          isSelected ? styles.active : "",
          isCurrent ? styles.currentChannel : "",
        ].filter(Boolean).join(" ")}
        style={{ paddingLeft: `${12 + indentPx}px` }}
        onClick={() => onSelectChannel(channel.id)}
        onDoubleClick={() => onJoinChannel(channel.id)}
        onContextMenu={(e) => onContextMenu(e, channel.id)}
      >
        <div className={styles.channelInfo}>
          <span className={styles.channelName}>
            {channel.name || "Root"}
            {isListened && (
              <span className={styles.listenIndicator} title="Listening">
                <ListenBadgeIcon width={12} height={12} />
              </span>
            )}
            <PchatBadge protocol={channel.pchat_protocol} />
          </span>
        </div>
        {unread > 0 && (
          <span className={styles.unreadBadge}>{unread > 99 ? "99+" : unread}</span>
        )}
        <StackedAvatars users={chUsers} />
      </button>
      </ChannelDropWrapper>
    );
  }

  return (
    <>
      {currentChannelEntry && (
        <div className={styles.stickyCurrentChannel}>
          <ChannelDropWrapper channelId={currentChannelEntry.id}>
          <button
            className={[styles.channelItem, styles.currentChannel].join(" ")}
            onClick={() => onSelectChannel(currentChannelEntry.id)}
            onDoubleClick={() => onJoinChannel(currentChannelEntry.id)}
            onContextMenu={(e) => onContextMenu(e, currentChannelEntry.id)}
          >
            <div className={styles.channelInfo}>
              <span className={styles.channelName}>{currentChannelEntry.name || "Root"}</span>
            </div>
            <StackedAvatars users={usersByChannel.get(currentChannelEntry.id) ?? []} />
          </button>
          </ChannelDropWrapper>
        </div>
      )}

      {root && root.id !== currentChannel && (
        <ChannelDropWrapper channelId={root.id}>
        <button
          className={[
            styles.channelItem,
            selectedChannel === root.id ? styles.active : "",
          ].filter(Boolean).join(" ")}
          onClick={() => onSelectChannel(root.id)}
          onDoubleClick={() => onJoinChannel(root.id)}
          onContextMenu={(e) => onContextMenu(e, root.id)}
        >
          <div className={styles.channelInfo}>
            <span className={styles.channelName}>{root.name || "Root"}</span>
          </div>
          <StackedAvatars users={usersByChannel.get(root.id) ?? []} />
        </button>
        </ChannelDropWrapper>
      )}

      {root && sortedDirectChildren(root.id).map((ch) => renderChannel(ch, 0))}
    </>
  );
}
