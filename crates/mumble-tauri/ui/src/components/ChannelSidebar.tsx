import { useState, useMemo, useEffect, useCallback, useRef } from "react";
import { createPortal } from "react-dom";
import { useNavigate } from "react-router-dom";
import { useAppStore } from "../store";
import type { ChannelEntry, UserEntry } from "../types";
import { textureToDataUrl, parseComment } from "../profileFormat";
import { ProfilePreviewCard } from "../pages/settings/ProfilePreviewCard";
import styles from "./ChannelSidebar.module.css";

const AVATAR_COLORS = [
  "#2AABEE",
  "#7c3aed",
  "#22c55e",
  "#f59e0b",
  "#ef4444",
  "#ec4899",
];

function colorFor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}

const MAX_STACKED = 3;

/**
 * Cache texture→dataURL conversions keyed by session ID.
 * Invalidated when the texture reference changes (length as a simple check).
 */
const textureCache = new Map<number, { len: number; url: string }>();

/** Get avatar data-URL from texture bytes, or null.  Uses a per-session cache. */
function avatarUrl(user: UserEntry): string | null {
  if (!user.texture || user.texture.length === 0) return null;
  const cached = textureCache.get(user.session);
  if (cached && cached.len === user.texture.length) return cached.url;
  const url = textureToDataUrl(user.texture);
  textureCache.set(user.session, { len: user.texture.length, url });
  return url;
}

// --- Stacked avatar component -------------------------------------

function StackedAvatars({ users }: { users: UserEntry[] }) {
  const [showTooltip, setShowTooltip] = useState(false);
  if (users.length === 0) return null;

  const visible = users.slice(0, MAX_STACKED);
  const overflow = users.length - MAX_STACKED;

  return (
    <div
      className={styles.stackedAvatars}
      onMouseEnter={() => setShowTooltip(true)}
      onMouseLeave={() => setShowTooltip(false)}
    >
      {visible.map((u, i) => {
        const url = avatarUrl(u);
        return (
          <div
            key={u.session}
            className={styles.stackedAvatar}
            style={{
              background: url ? "transparent" : colorFor(u.name),
              zIndex: MAX_STACKED - i,
            }}
          >
            {url ? (
              <img src={url} alt={u.name} className={styles.stackedAvatarImg} />
            ) : (
              u.name.charAt(0).toUpperCase()
            )}
          </div>
        );
      })}
      {overflow > 0 && (
        <div className={`${styles.stackedAvatar} ${styles.overflowBadge}`}>
          +{overflow}
        </div>
      )}
      {showTooltip && (
        <div className={styles.avatarTooltip}>
          {users.map((u) => {
            const url = avatarUrl(u);
            return (
              <div key={u.session} className={styles.tooltipUser}>
                {url ? (
                  <img src={url} alt={u.name} className={styles.tooltipAvatarImg} />
                ) : (
                  <div
                    className={styles.tooltipAvatar}
                    style={{ background: colorFor(u.name) }}
                  >
                    {u.name.charAt(0).toUpperCase()}
                  </div>
                )}
                <span>{u.name}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// --- Build tree helpers -------------------------------------------

// --- User item with profile card on hover -------------------------

function UserItem({ user, channelName: chName }: { user: UserEntry; channelName: string }) {
  const [showCard, setShowCard] = useState(false);
  const [cardPos, setCardPos] = useState<{ top: number; left: number } | null>(null);
  const itemRef = useRef<HTMLDivElement>(null);
  const selectUser = useAppStore((s) => s.selectUser);
  const url = useMemo(() => avatarUrl(user), [user.texture]);
  const parsed = useMemo(
    () => (user.comment ? parseComment(user.comment) : null),
    [user.comment],
  );

  const handleEnter = useCallback(() => {
    if (itemRef.current) {
      const rect = itemRef.current.getBoundingClientRect();
      // Position the card to the right of the sidebar, vertically centred on the row.
      setCardPos({
        top: rect.top + rect.height / 2,
        left: rect.right + 8,
      });
    }
    setShowCard(true);
  }, []);

  const handleLeave = useCallback(() => {
    setShowCard(false);
  }, []);

  const handleClick = useCallback(() => {
    selectUser(user.session);
  }, [user.session, selectUser]);

  return (
    <div
      ref={itemRef}
      className={styles.userItem}
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
      onClick={handleClick}
    >
      <div className={styles.userAvatarWrap}>
        {url ? (
          <img src={url} alt={user.name} className={styles.userAvatarImg} />
        ) : (
          <div
            className={styles.userAvatar}
            style={{ background: colorFor(user.name) }}
          >
            {user.name.charAt(0).toUpperCase()}
          </div>
        )}
        <span className={styles.onlineDot} />
      </div>
      <span className={styles.userName}>{user.name}</span>
      <span className={styles.userChannelChip}>{chName}</span>
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
          />
        </div>,
        document.body,
      )}
    </div>
  );
}

// --- Build tree helpers (continued) ------------------------------

interface ChannelGroup {
  folder: ChannelEntry;
  /** All channels recursively under this folder, flattened. */
  children: ChannelEntry[];
}

function buildGroups(channels: ChannelEntry[]): {
  root: ChannelEntry | null;
  groups: ChannelGroup[];
} {
  // Find the root channel (parent_id === null or parent is self, usually id 0).
  const root =
    channels.find((c) => c.parent_id === null || c.parent_id === c.id) ?? null;
  const rootId = root?.id ?? 0;

  // Direct children of root are "main folders".
  const topLevel = channels.filter(
    (c) => c.parent_id === rootId && c.id !== rootId,
  );

  // For each top-level folder, collect ALL descendants recursively, flattened.
  function collectDescendants(parentId: number): ChannelEntry[] {
    const result: ChannelEntry[] = [];
    for (const ch of channels) {
      if (ch.parent_id === parentId && ch.id !== parentId) {
        result.push(ch);
        result.push(...collectDescendants(ch.id));
      }
    }
    return result;
  }

  const groups: ChannelGroup[] = topLevel.map((folder) => ({
    folder,
    children: collectDescendants(folder.id),
  }));

  return { root, groups };
}

// --- Main component -----------------------------------------------

interface ChannelSidebarProps {
  /** Called after the user taps a channel (used by mobile drawer to close). */
  onChannelSelect?: () => void;
}

export default function ChannelSidebar({ onChannelSelect }: ChannelSidebarProps) {
  const channels = useAppStore((s) => s.channels);
  const users = useAppStore((s) => s.users);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const currentChannel = useAppStore((s) => s.currentChannel);
  const selectChannel = useAppStore((s) => s.selectChannel);
  const joinChannel = useAppStore((s) => s.joinChannel);
  const disconnect = useAppStore((s) => s.disconnect);
  const toggleListen = useAppStore((s) => s.toggleListen);
  const listenedChannels = useAppStore((s) => s.listenedChannels);
  const unreadCounts = useAppStore((s) => s.unreadCounts);
  const voiceState = useAppStore((s) => s.voiceState);
  const toggleMute = useAppStore((s) => s.toggleMute);
  const toggleDeafen = useAppStore((s) => s.toggleDeafen);
  const navigate = useNavigate();

  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  // -- Context menu state ------------------------------------------
  const [ctxMenu, setCtxMenu] = useState<{
    x: number;
    y: number;
    channelId: number;
  } | null>(null);
  const ctxRef = useRef<HTMLDivElement>(null);

  const openCtxMenu = useCallback(
    (e: React.MouseEvent, channelId: number) => {
      e.preventDefault();
      e.stopPropagation();
      setCtxMenu({ x: e.clientX, y: e.clientY, channelId });
    },
    [],
  );

  // Close context menu on outside click or Escape.
  useEffect(() => {
    if (!ctxMenu) return;
    const handleClick = (e: MouseEvent) => {
      if (ctxRef.current && !ctxRef.current.contains(e.target as Node)) {
        setCtxMenu(null);
      }
    };
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setCtxMenu(null);
    };
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("keydown", handleKey);
    return () => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("keydown", handleKey);
    };
  }, [ctxMenu]);

  const { root, groups } = useMemo(() => buildGroups(channels), [channels]);

  const usersByChannel = useMemo(() => {
    const map = new Map<number, UserEntry[]>();
    for (const u of users) {
      const list = map.get(u.channel_id) ?? [];
      list.push(u);
      map.set(u.channel_id, list);
    }
    return map;
  }, [users]);

  /** Count all users in a folder and its descendants. */
  const groupUserCount = useCallback(
    (group: ChannelGroup) => {
      let count = usersByChannel.get(group.folder.id)?.length ?? 0;
      for (const ch of group.children) {
        count += usersByChannel.get(ch.id)?.length ?? 0;
      }
      return count;
    },
    [usersByChannel],
  );

  /** Collect all users from a folder and its descendants for stacked avatars. */
  const groupUsers = useCallback(
    (group: ChannelGroup) => {
      const all: UserEntry[] = [];
      const folderUsers = usersByChannel.get(group.folder.id);
      if (folderUsers) all.push(...folderUsers);
      for (const ch of group.children) {
        const chUsers = usersByChannel.get(ch.id);
        if (chUsers) all.push(...chUsers);
      }
      return all;
    },
    [usersByChannel],
  );

  // Sort groups: populated first, then alphabetical within each tier.
  const sortedGroups = useMemo(() => {
    return [...groups].sort((a, b) => {
      const aCount = groupUserCount(a);
      const bCount = groupUserCount(b);
      // Populated channels first.
      if (aCount > 0 && bCount === 0) return -1;
      if (aCount === 0 && bCount > 0) return 1;
      // Same tier → alphabetical.
      return a.folder.name.localeCompare(b.folder.name);
    });
  }, [groups, groupUserCount]);

  /** Sort a group's children so channels with users appear first. */
  const sortedChildren = useCallback(
    (children: ChannelEntry[]) =>
      [...children].sort((a, b) => {
        const aUsers = usersByChannel.get(a.id)?.length ?? 0;
        const bUsers = usersByChannel.get(b.id)?.length ?? 0;
        if (aUsers > 0 && bUsers === 0) return -1;
        if (aUsers === 0 && bUsers > 0) return 1;
        return a.name.localeCompare(b.name);
      }),
    [usersByChannel],
  );

  /** Find the ChannelEntry for the user's current channel. */
  const currentChannelEntry = useMemo(
    () => (currentChannel == null ? null : channels.find((c) => c.id === currentChannel) ?? null),
    [channels, currentChannel],
  );

  /** Get the channel name for a user's current channel. */
  const channelName = (channelId: number) => {
    const ch = channels.find((c) => c.id === channelId);
    return ch?.name || "Root";
  };

  const toggleExpand = (id: number) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  // -- Channel item renderer ---------------------------------------

  function renderChannelItem(
    channel: ChannelEntry,
    indent = false,
    highlight = false,
  ) {
    const chUsers = usersByChannel.get(channel.id) ?? [];
    const unread = unreadCounts[channel.id] ?? 0;
    const isListened = listenedChannels.has(channel.id);
    return (
      <button
        key={channel.id}
        className={`${styles.channelItem} ${indent ? styles.indented : ""} ${
          selectedChannel === channel.id ? styles.active : ""
        } ${highlight ? styles.currentChannel : ""}`}
        onClick={() => { selectChannel(channel.id); onChannelSelect?.(); }}
        onDoubleClick={() => { joinChannel(channel.id); onChannelSelect?.(); }}
        onContextMenu={(e) => openCtxMenu(e, channel.id)}
      >
        <div className={styles.channelInfo}>
          <span className={styles.channelName}>
            {channel.name || "Root"}
            {isListened && (
              <span className={styles.listenIndicator} title="Listening">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M12 3a9 9 0 0 0-9 9v7c0 1.1.9 2 2 2h4v-8H5v-1a7 7 0 0 1 14 0v1h-4v8h4c1.1 0 2-.9 2-2v-7a9 9 0 0 0-9-9z"/>
                </svg>
              </span>
            )}
          </span>
        </div>
        {unread > 0 && (
          <span className={styles.unreadBadge}>
            {unread > 99 ? "99+" : unread}
          </span>
        )}
        <StackedAvatars users={chUsers} />
      </button>
    );
  }

  return (
    <aside className={styles.sidebar}>
      {/* Header */}
      <div className={styles.header}>
        <h2 className={styles.headerTitle}>Channels</h2>
        <button
          className={styles.disconnectBtn}
          onClick={disconnect}
          title="Disconnect"
        >
          <svg
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
            <polyline points="16 17 21 12 16 7" />
            <line x1="21" y1="12" x2="9" y2="12" />
          </svg>
        </button>
      </div>

      {/* Sticky current channel */}
      {currentChannelEntry && (
        <div className={styles.stickyCurrentChannel}>
          {renderChannelItem(currentChannelEntry, false, true)}
        </div>
      )}

      {/* Channel list */}
      <div className={styles.channelList}>
        {/* Root channel */}
        {root && root.id !== currentChannel && renderChannelItem(root)}

        {/* Grouped main folders - sorted: populated first */}
        {sortedGroups.map((group) => {
          const isOpen = expanded.has(group.folder.id);
          const totalUsers = groupUserCount(group);
          const allGroupUsers = groupUsers(group);
          const folderUnread = unreadCounts[group.folder.id] ?? 0;
          const isFolderListened = listenedChannels.has(group.folder.id);
          const isCurrent = group.folder.id === currentChannel;

          return (
            <div key={group.folder.id} className={styles.folderGroup}>
              <div
                className={`${styles.folderHeader} ${
                  selectedChannel === group.folder.id ? styles.active : ""
                } ${isCurrent ? styles.currentChannel : ""}`}
                onContextMenu={(e) => openCtxMenu(e, group.folder.id)}
              >
                <button
                  className={styles.expandBtn}
                  onClick={() => toggleExpand(group.folder.id)}
                  aria-label={isOpen ? "Collapse" : "Expand"}
                >
                  <svg
                    className={`${styles.chevron} ${isOpen ? styles.chevronOpen : ""}`}
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <polyline points="9 18 15 12 9 6" />
                  </svg>
                </button>
                <button
                  className={styles.folderSelect}
                  onClick={() => selectChannel(group.folder.id)}
                  onDoubleClick={() => joinChannel(group.folder.id)}
                >
                  <span className={styles.channelName}>
                    {group.folder.name || "Unnamed"}
                    {isFolderListened && (
                      <span className={styles.listenIndicator} title="Listening">
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
                          <path d="M12 3a9 9 0 0 0-9 9v7c0 1.1.9 2 2 2h4v-8H5v-1a7 7 0 0 1 14 0v1h-4v8h4c1.1 0 2-.9 2-2v-7a9 9 0 0 0-9-9z"/>
                        </svg>
                      </span>
                    )}
                  </span>
                  <span className={styles.channelMeta}>
                    {totalUsers} {totalUsers === 1 ? "member" : "members"}
                  </span>
                </button>
                {folderUnread > 0 && (
                  <span className={styles.unreadBadge}>
                    {folderUnread > 99 ? "99+" : folderUnread}
                  </span>
                )}
                <StackedAvatars users={allGroupUsers} />
              </div>

              {isOpen && group.children.length > 0 && (
                <div className={styles.folderChildren}>
                  {sortedChildren(group.children).map((ch) =>
                    renderChannelItem(
                      ch,
                      true,
                      ch.id === currentChannel,
                    ),
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>

      <div className={styles.divider} />

      {/* Online users */}
      <div className={styles.userSection}>
        <h3 className={styles.sectionTitle}>Online - {users.length}</h3>
        <div className={styles.userList}>
          {users.map((user) => (
            <UserItem
              key={user.session}
              user={user}
              channelName={channelName(user.channel_id)}
            />
          ))}
        </div>
      </div>

      {/* Voice panel */}
      <div className={styles.voicePanel}>
        <div className={styles.voiceInfo}>
          {voiceState === "active" ? (
            <>
              <span className={styles.voiceDot} />
              <span className={styles.voiceLabel}>Voice Connected</span>
            </>
          ) : voiceState === "muted" ? (
            <span className={styles.voiceLabelMuted}>Muted</span>
          ) : (
            <span className={styles.voiceLabelMuted}>Deaf &amp; Muted</span>
          )}
        </div>

        <div className={styles.voiceActions}>
          {currentChannel != null && (
            <>
              {/* Mute toggle */}
              <button
                className={`${styles.voiceToggle} ${voiceState === "active" ? styles.voiceActive : ""}`}
                onClick={toggleMute}
                title={voiceState === "active" ? "Mute" : voiceState === "muted" ? "Unmute" : "Mute"}
              >
                {voiceState !== "muted" ? (
                  /* Mic on icon */
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
                    <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                    <line x1="12" y1="19" x2="12" y2="23" />
                    <line x1="8" y1="23" x2="16" y2="23" />
                  </svg>
                ) : (
                  /* Mic off icon */
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <line x1="1" y1="1" x2="23" y2="23" />
                    <path d="M9 9v3a3 3 0 0 0 5.12 2.12M15 9.34V4a3 3 0 0 0-5.94-.6" />
                    <path d="M17 16.95A7 7 0 0 1 5 12v-2m14 0v2c0 .76-.12 1.5-.35 2.18" />
                    <line x1="12" y1="19" x2="12" y2="23" />
                    <line x1="8" y1="23" x2="16" y2="23" />
                  </svg>
                )}
              </button>

              {/* Deafen toggle */}
              <button
                className={`${styles.voiceToggle} ${voiceState === "inactive" ? "" : styles.voiceActive}`}
                onClick={toggleDeafen}
                title={voiceState === "inactive" ? "Undeafen" : "Deafen"}
              >
                {voiceState === "inactive" ? (
                  /* Headphone off icon */
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <line x1="1" y1="1" x2="23" y2="23" />
                    <path d="M4.53 4.53A9 9 0 0 0 3 12v7c0 1.1.9 2 2 2h4v-8H5.07" />
                    <path d="M21 12a9 9 0 0 0-15.47-6.27" />
                    <path d="M15 21h4c1.1 0 2-.9 2-2v-7" />
                  </svg>
                ) : (
                  /* Headphone on icon */
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M3 18v-6a9 9 0 0 1 18 0v6" />
                    <path d="M21 19a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3v5z" />
                    <path d="M3 19a2 2 0 0 0 2 2h1a2 2 0 0 0 2-2v-3a2 2 0 0 0-2-2H3v5z" />
                  </svg>
                )}
              </button>
            </>
          )}

          <button
            className={styles.settingsBtn}
            onClick={() => navigate("/settings")}
            title="Audio settings"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="3" />
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
            </svg>
          </button>
        </div>
      </div>

      {/* Context menu */}
      {ctxMenu && (
        <div
          ref={ctxRef}
          className={styles.contextMenu}
          style={{ top: ctxMenu.y, left: ctxMenu.x }}
        >
          <button
            className={styles.contextMenuItem}
            onClick={() => {
              toggleListen(ctxMenu.channelId);
              setCtxMenu(null);
            }}
          >
            {listenedChannels.has(ctxMenu.channelId) ? (
              <>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <line x1="1" y1="1" x2="23" y2="23" />
                  <path d="M9 9v3a3 3 0 0 0 5.12 2.12M15 9.34V4a3 3 0 0 0-5.94-.6" />
                  <path d="M17 16.95A7 7 0 0 1 5 12v-2m14 0v2c0 .76-.12 1.5-.35 2.18" />
                </svg>
                Stop listening
              </>
            ) : (
              <>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M12 3a9 9 0 0 0-9 9v7c0 1.1.9 2 2 2h4v-8H5v-1a7 7 0 0 1 14 0v1h-4v8h4c1.1 0 2-.9 2-2v-7a9 9 0 0 0-9-9z"/>
                </svg>
                Listen to channel
              </>
            )}
          </button>
        </div>
      )}
    </aside>
  );
}
