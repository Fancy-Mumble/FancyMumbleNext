import { useState, useMemo, useEffect, useCallback, useRef } from "react";
import { createPortal } from "react-dom";
import { useNavigate } from "react-router-dom";
import { useAppStore } from "../store";
import type { ChannelEntry, UserEntry, SidebarSections } from "../types";
import { getPreferences, updatePreferences } from "../preferencesStorage";
import { SidebarSearchView } from "./SidebarSearchView";
import { UserListItem, colorFor, avatarUrl } from "./UserListItem";
import { UserContextMenu } from "./UserContextMenu";
import type { UserContextMenuState } from "./UserContextMenu";
import ChannelEditorDialog, { canEditChannel, canCreateChannel, canOnlyCreateTemp, canDeleteChannel } from "./ChannelEditorDialog";
import styles from "./ChannelSidebar.module.css";

/** Mumble permission bitmask: Listen to channel (bit 11). */
const PERM_LISTEN = 0x800;

/** Mumble permission bitmask: Write / admin (bit 0). */
const PERM_WRITE = 0x01;

/** Check whether a channel's cached permissions include the Listen bit. */
function canListen(channel: ChannelEntry | undefined): boolean {
  if (!channel) return true; // channel not found - allow optimistically
  if (channel.permissions == null) return true; // not yet queried - allow optimistically
  return (channel.permissions & PERM_LISTEN) !== 0;
}

const MAX_STACKED = 3;

// --- Stacked avatar component -------------------------------------

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
        result.push(ch, ...collectDescendants(ch.id));
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

// --- Group creation modal -----------------------------------------

interface GroupCreateModalProps {
  readonly users: UserEntry[];
  readonly ownSession: number | null;
  readonly onClose: () => void;
  readonly onCreate: (name: string, members: number[]) => Promise<void>;
}

function GroupCreateModal({ users, ownSession, onClose, onCreate }: GroupCreateModalProps) {
  const [name, setName] = useState("");
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [creating, setCreating] = useState(false);
  const backdropRef = useRef<HTMLDivElement>(null);

  const otherUsers = useMemo(
    () => users.filter((u) => u.session !== ownSession),
    [users, ownSession],
  );

  // Close on Escape key.
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onClose]);

  const toggleMember = useCallback((session: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(session)) next.delete(session);
      else next.add(session);
      return next;
    });
  }, []);

  const handleCreate = useCallback(async () => {
    if (selected.size === 0 || !name.trim()) return;
    setCreating(true);
    try {
      await onCreate(name.trim(), Array.from(selected));
    } finally {
      setCreating(false);
    }
  }, [name, selected, onCreate]);

  const handleBackdropClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === backdropRef.current) onClose();
    },
    [onClose],
  );

  return createPortal(
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div
      ref={backdropRef}
      className={styles.modalBackdrop}
      onClick={handleBackdropClick}
      onKeyDown={(e) => { if (e.key === "Escape") onClose(); }}
    >
      <div className={styles.modalContent}>
        <h3 className={styles.modalTitle}>New Group Chat</h3>

        <input
          className={styles.modalInput}
          type="text"
          placeholder="Group name..."
          value={name}
          onChange={(e) => setName(e.target.value)}
          autoFocus
        />

        <p className={styles.modalSubtitle}>Select members:</p>

        <div className={styles.modalUserList}>
          {otherUsers.map((u) => {
            const isSelected = selected.has(u.session);
            const url = avatarUrl(u);
            return (
              <button
                key={u.session}
                type="button"
                className={`${styles.modalUserItem} ${isSelected ? styles.modalUserSelected : ""}`}
                onClick={() => toggleMember(u.session)}
              >
                <div className={styles.modalCheckbox}>
                  {isSelected && (
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                  )}
                </div>
                {url ? (
                  <img src={url} alt={u.name} className={styles.userAvatarImg} style={{ width: 24, height: 24 }} />
                ) : (
                  <div
                    className={styles.userAvatar}
                    style={{ background: colorFor(u.name), width: 24, height: 24, fontSize: 11 }}
                  >
                    {u.name.charAt(0).toUpperCase()}
                  </div>
                )}
                <span className={styles.userName}>{u.name}</span>
              </button>
            );
          })}
          {otherUsers.length === 0 && (
            <p className={styles.modalEmpty}>No other users online</p>
          )}
        </div>

        <div className={styles.modalActions}>
          <button className={styles.modalCancelBtn} onClick={onClose}>
            Cancel
          </button>
          <button
            className={styles.modalCreateBtn}
            onClick={handleCreate}
            disabled={creating || selected.size === 0 || !name.trim()}
          >
            {creating ? "Creating..." : `Create (${selected.size} selected)`}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// --- Voice panel helpers -------------------------------------------

// --- Main component -----------------------------------------------

interface ChannelSidebarProps {
  /** Called after the user taps a channel (used by mobile drawer to close). */
  onChannelSelect?: () => void;
  /** Toggle the server info panel. */
  onServerInfoToggle?: () => void;
  /** Called when the user clicks the collapse button (desktop narrow mode). */
  onCollapse?: () => void;
  /** When set, opens search scoped to this channel. */
  searchChannelId?: number | null;
  /** Called to clear the channel search scope. */
  onSearchChannelClear?: () => void;
}

export default function ChannelSidebar({ onChannelSelect, onServerInfoToggle, onCollapse, searchChannelId, onSearchChannelClear }: Readonly<ChannelSidebarProps>) {
  const channels = useAppStore((s) => s.channels);
  const users = useAppStore((s) => s.users);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const currentChannel = useAppStore((s) => s.currentChannel);
  const selectChannel = useAppStore((s) => s.selectChannel);
  const joinChannel = useAppStore((s) => s.joinChannel);
  const deleteChannel = useAppStore((s) => s.deleteChannel);
  const disconnect = useAppStore((s) => s.disconnect);
  const toggleListen = useAppStore((s) => s.toggleListen);
  const listenedChannels = useAppStore((s) => s.listenedChannels);
  const unreadCounts = useAppStore((s) => s.unreadCounts);
  const voiceState = useAppStore((s) => s.voiceState);
  const toggleMute = useAppStore((s) => s.toggleMute);
  const toggleDeafen = useAppStore((s) => s.toggleDeafen);
  const navigate = useNavigate();

  // Group chat state
  const groupChats = useAppStore((s) => s.groupChats);
  const selectedGroup = useAppStore((s) => s.selectedGroup);
  const selectGroup = useAppStore((s) => s.selectGroup);
  const groupUnreadCounts = useAppStore((s) => s.groupUnreadCounts);
  const createGroup = useAppStore((s) => s.createGroup);
  const ownSession = useAppStore((s) => s.ownSession);

  const selectDmUser = useAppStore((s) => s.selectDmUser);
  const selectUser = useAppStore((s) => s.selectUser);
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);

  const [showGroupModal, setShowGroupModal] = useState(false);
  const [showSearch, setShowSearch] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const searchInputRef = useRef<HTMLInputElement>(null);

  // -- Channel editor dialog state --------------------------------
  const [channelEditor, setChannelEditor] = useState<{
    channel: ChannelEntry | null;
    parentId: number;
    tempOnly: boolean;
  } | null>(null);

  // -- Delete channel confirm state --------------------------------
  const [deleteConfirm, setDeleteConfirm] = useState<{
    channelId: number;
    channelName: string;
  } | null>(null);

  // True when the user has Write permission on the root channel (id 0).
  // This is the traditional Mumble indicator for server admin rights.
  const isAdmin = useMemo(() => {
    const root = channels.find((ch) => ch.id === 0);
    return root?.permissions != null && (root.permissions & PERM_WRITE) !== 0;
  }, [channels]);

  // Global Ctrl+K / Cmd+K shortcut to toggle sidebar search.
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "k") {
        e.preventDefault();
        if (showSearch) {
          setShowSearch(false);
          setSearchQuery("");
        } else {
          setShowSearch(true);
          requestAnimationFrame(() => searchInputRef.current?.focus());
        }
      }
    };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [showSearch]);

  const closeSearch = useCallback(() => {
    setShowSearch(false);
    setSearchQuery("");
    onSearchChannelClear?.();
  }, [onSearchChannelClear]);

  // Open search when a channel search is requested from the chat header.
  useEffect(() => {
    if (searchChannelId != null) {
      setShowSearch(true);
      requestAnimationFrame(() => searchInputRef.current?.focus());
    }
  }, [searchChannelId]);

  // Resolve channel name for the search scope indicator.
  const searchChannelName = useMemo(() => {
    if (searchChannelId == null) return undefined;
    return channels.find((ch) => ch.id === searchChannelId)?.name;
  }, [searchChannelId, channels]);

  // Section collapse state (all expanded by default, restored from prefs).
  const [channelsOpen, setChannelsOpen] = useState(true);
  const [groupsOpen, setGroupsOpen] = useState(true);
  const [onlineOpen, setOnlineOpen] = useState(true);

  // Load persisted section states on mount.
  useEffect(() => {
    getPreferences().then((prefs) => {
      const s = prefs.sidebarSections;
      if (s) {
        setChannelsOpen(s.channels);
        setGroupsOpen(s.groups);
        setOnlineOpen(s.online);
      }
    });
  }, []);

  // Persist section states when they change.
  const toggleSection = useCallback(
    (section: keyof SidebarSections, current: boolean, setter: (v: boolean) => void) => {
      const next = !current;
      setter(next);
      getPreferences().then((prefs) => {
        const sections = prefs.sidebarSections ?? { channels: true, groups: true, online: true };
        updatePreferences({ sidebarSections: { ...sections, [section]: next } });
      });
    },
    [],
  );

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

  // -- User context menu state ------------------------------------
  const [userCtxMenu, setUserCtxMenu] = useState<UserContextMenuState | null>(null);

  const openUserCtxMenu = useCallback(
    (e: React.MouseEvent, user: UserEntry) => {
      e.preventDefault();
      e.stopPropagation();
      setUserCtxMenu({ x: e.clientX, y: e.clientY, user });
    },
    [],
  );

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
      // Same tier -> alphabetical.
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

  // Computed display values to avoid nested ternaries in JSX.
  const isVoiceActive = voiceState === "active";
  const isVoiceInactive = voiceState === "inactive";
  const muteTitle = isVoiceActive ? "Mute" : "Unmute";

  return (
    <aside className={styles.sidebar}>
      {/* Header */}
      <div className={styles.header}>
        {onCollapse && (
          <button
            type="button"
            className={styles.collapseBtn}
            onClick={onCollapse}
            aria-label="Collapse sidebar"
            title="Collapse sidebar"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="3" y1="12" x2="21" y2="12" />
              <line x1="3" y1="6" x2="21" y2="6" />
              <line x1="3" y1="18" x2="21" y2="18" />
            </svg>
          </button>
        )}
        <div className={styles.searchBar}>
          <svg
            className={styles.searchBarIcon}
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
          </svg>
          <input
            ref={searchInputRef}
            className={styles.searchBarInput}
            type="text"
            placeholder="Search..."
            value={searchQuery}
            onChange={(e) => {
              setSearchQuery(e.target.value);
              if (!showSearch) setShowSearch(true);
            }}
            onFocus={() => { if (!showSearch) setShowSearch(true); }}
            onKeyDown={(e) => { if (e.key === "Escape") closeSearch(); }}
          />
          {showSearch ? (
            <button
              type="button"
              className={styles.searchBarClose}
              onClick={closeSearch}
              aria-label="Close search"
              title="Close search (Esc)"
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          ) : (
            <span className={styles.searchShortcut}>Ctrl+K</span>
          )}
        </div>
      </div>

      {/* -- Search mode replaces channel/group/online content -- */}
      {showSearch ? (
        <SidebarSearchView
          query={searchQuery}
          channelId={searchChannelId}
          channelName={searchChannelName}
          onSelectChannel={(id) => { selectChannel(id); onChannelSelect?.(); }}
          onSelectUser={(session) => { selectDmUser(session); onChannelSelect?.(); }}
          onSelectGroup={(id) => { selectGroup(id); onChannelSelect?.(); }}
        />
      ) : (<>

      {/* Sticky current channel */}
      {currentChannelEntry && (
        <div className={styles.stickyCurrentChannel}>
          {renderChannelItem(currentChannelEntry, false, true)}
        </div>
      )}

      {/* Channel list header (always visible) */}
      <div className={styles.sectionHeaderBar}>
        <button
          className={styles.collapsibleHeader}
          onClick={() => toggleSection("channels", channelsOpen, setChannelsOpen)}
          type="button"
        >
          <svg
            className={`${styles.collapseChevron} ${channelsOpen ? styles.collapseChevronOpen : ""}`}
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <polyline points="9 18 15 12 9 6" />
          </svg>
          <span>Channels</span>
        </button>
      </div>

      {/* Channel list */}
      <div className={`${styles.channelList} ${channelsOpen ? "" : styles.sectionCollapsed}`}>

        {channelsOpen && (<>
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
                {group.children.length > 0 && (
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
                )}
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
        </>)}
      </div>

      <div className={styles.divider} />

      {/* Group chats */}
      <div className={`${styles.userSection} ${groupsOpen ? "" : styles.sectionCollapsed}`}>
        <div className={styles.groupSectionHeader}>
          <button
            className={styles.collapsibleHeader}
            onClick={() => toggleSection("groups", groupsOpen, setGroupsOpen)}
            type="button"
          >
            <svg
              className={`${styles.collapseChevron} ${groupsOpen ? styles.collapseChevronOpen : ""}`}
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <polyline points="9 18 15 12 9 6" />
            </svg>
            <span>
              Group Chats{groupChats.length > 0 ? ` - ${groupChats.length}` : ""}
            </span>
          </button>
          <button
            className={styles.newGroupBtn}
            onClick={() => setShowGroupModal(true)}
            title="New group chat"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
          </button>
        </div>
        {groupsOpen && <div className={styles.userList}>
          {groupChats.map((group) => {
            const unread = groupUnreadCounts[group.id] ?? 0;
            const isActive = selectedGroup === group.id;
            const memberNames = group.members
              .map((s) => users.find((u) => u.session === s)?.name)
              .filter(Boolean)
              .join(", ");
            return (
              <button
                key={group.id}
                type="button"
                className={`${styles.userItem} ${isActive ? styles.userItemActive : ""}`}
                onClick={() => { selectGroup(group.id); onChannelSelect?.(); }}
                title={memberNames || group.name}
              >
                <div className={styles.userAvatarWrap}>
                  <div
                    className={styles.userAvatar}
                    style={{ background: colorFor(group.name) }}
                  >
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
                      <circle cx="9" cy="7" r="4" />
                      <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
                      <path d="M16 3.13a4 4 0 0 1 0 7.75" />
                    </svg>
                  </div>
                </div>
                <span className={styles.userName}>{group.name}</span>
                {unread > 0 && (
                  <span className={styles.unreadBadge}>
                    {unread > 99 ? "99+" : unread}
                  </span>
                )}
                <span className={styles.userChannelChip}>
                  {group.members.length} {group.members.length === 1 ? "member" : "members"}
                </span>
              </button>
            );
          })}
        </div>}
      </div>

      <div className={styles.divider} />

      {/* Online users */}
      <div className={`${styles.userSection} ${onlineOpen ? "" : styles.sectionCollapsed}`}>
        <button
          className={styles.collapsibleHeader}
          onClick={() => toggleSection("online", onlineOpen, setOnlineOpen)}
          type="button"
        >
          <svg
            className={`${styles.collapseChevron} ${onlineOpen ? styles.collapseChevronOpen : ""}`}
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <polyline points="9 18 15 12 9 6" />
          </svg>
          <span>Online - {users.length}</span>
        </button>
        {onlineOpen && <>
          <div className={styles.userList}>
            {users.filter((u) => u.session !== ownSession).map((user) => (
              <UserListItem
                key={user.session}
                user={user}
                channelName={channelName(user.channel_id)}
                active={selectedDmUser === user.session}
                onClick={() => selectDmUser(user.session)}
                onContextMenu={(e) => openUserCtxMenu(e, user)}
              />
            ))}
          </div>
        </>}
      </div>

      </>)}{/* end search-mode ternary */}

      {/* Self user section - always visible */}
      {(() => {
        const self = users.find((u) => u.session === ownSession);
        if (!self) return null;
        return (
          <div className={styles.selfUserSection}>
            <UserListItem
              user={self}
              channelName={channelName(self.channel_id)}
              isSelf
              onClick={() => selectUser(self.session)}
              onContextMenu={(e) => openUserCtxMenu(e, self)}
            />
            {currentChannel != null && (
              <div className={styles.selfVoiceActions}>
                <button
                  className={`${styles.voiceToggle} ${isVoiceActive ? styles.voiceActive : ""}`}
                  onClick={toggleMute}
                  title={muteTitle}
                >
                  {isVoiceActive ? (
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
                      <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                      <line x1="12" y1="19" x2="12" y2="23" />
                      <line x1="8" y1="23" x2="16" y2="23" />
                    </svg>
                  ) : (
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <line x1="1" y1="1" x2="23" y2="23" />
                      <path d="M9 9v3a3 3 0 0 0 5.12 2.12M15 9.34V4a3 3 0 0 0-5.94-.6" />
                      <path d="M17 16.95A7 7 0 0 1 5 12v-2m14 0v2c0 .76-.12 1.5-.35 2.18" />
                      <line x1="12" y1="19" x2="12" y2="23" />
                      <line x1="8" y1="23" x2="16" y2="23" />
                    </svg>
                  )}
                </button>
                <button
                  className={`${styles.voiceToggle} ${isVoiceInactive ? "" : styles.voiceActive}`}
                  onClick={toggleDeafen}
                  title={isVoiceInactive ? "Enable Voice" : "Disable Voice"}
                >
                  {isVoiceInactive ? (
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <line x1="1" y1="1" x2="23" y2="23" />
                      <path d="M4.53 4.53A9 9 0 0 0 3 12v7c0 1.1.9 2 2 2h4v-8H5.07" />
                      <path d="M21 12a9 9 0 0 0-15.47-6.27" />
                      <path d="M15 21h4c1.1 0 2-.9 2-2v-7" />
                    </svg>
                  ) : (
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M3 18v-6a9 9 0 0 1 18 0v6" />
                      <path d="M21 19a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3v5z" />
                      <path d="M3 19a2 2 0 0 0 2 2h1a2 2 0 0 0 2-2v-3a2 2 0 0 0-2-2H3v5z" />
                    </svg>
                  )}
                </button>
              </div>
            )}
          </div>
        );
      })()}

      {/* Voice panel */}
      <div className={styles.voicePanel}>
        <div className={styles.voiceActions}>
          {onServerInfoToggle && (
            <button
              className={styles.serverInfoBtn}
              onClick={onServerInfoToggle}
              title="Server info"
              aria-label="Server info"
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="10" />
                <line x1="12" y1="16" x2="12" y2="12" />
                <line x1="12" y1="8" x2="12.01" y2="8" />
              </svg>
            </button>
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
          {isAdmin && (
            <button
              className={styles.adminBtn}
              onClick={() => navigate("/admin")}
              title="Admin panel"
              aria-label="Admin panel"
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
              </svg>
            </button>
          )}
          <button
            className={styles.disconnectBtn}
            onClick={disconnect}
            title="Disconnect"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
              <polyline points="16 17 21 12 16 7" />
              <line x1="21" y1="12" x2="9" y2="12" />
            </svg>
            Disconnect
          </button>
        </div>
      </div>

      {/* Context menu */}
      {ctxMenu && (() => {
        const ctxChannel = channels.find((c) => c.id === ctxMenu.channelId);
        const hasListenPerm = canListen(ctxChannel);
        const isListened = listenedChannels.has(ctxMenu.channelId);
        const showEdit = canEditChannel(ctxChannel);
        const showCreate = canCreateChannel(ctxChannel);
        const showDelete = canDeleteChannel(ctxChannel);

        return createPortal(
          <div
            ref={ctxRef}
            className={styles.contextMenu}
            style={{ top: ctxMenu.y, left: ctxMenu.x }}
          >
            <button
              className={styles.contextMenuItem}
              disabled={!isListened && !hasListenPerm}
              title={!isListened && !hasListenPerm ? "You do not have permission to listen to this channel" : undefined}
              onClick={() => {
                toggleListen(ctxMenu.channelId);
                setCtxMenu(null);
              }}
            >
              {isListened ? (
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
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" opacity={hasListenPerm ? 1 : 0.4}>
                    <path d="M12 3a9 9 0 0 0-9 9v7c0 1.1.9 2 2 2h4v-8H5v-1a7 7 0 0 1 14 0v1h-4v8h4c1.1 0 2-.9 2-2v-7a9 9 0 0 0-9-9z"/>
                  </svg>
                  Listen to channel
                </>
              )}
            </button>

            {showEdit && (
              <button
                className={styles.contextMenuItem}
                onClick={() => {
                  if (ctxChannel) {
                    setChannelEditor({ channel: ctxChannel, parentId: ctxChannel.parent_id ?? 0, tempOnly: false });
                  }
                  setCtxMenu(null);
                }}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
                  <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
                </svg>
                Edit Channel
              </button>
            )}

            {showCreate && (
              <button
                className={styles.contextMenuItem}
                onClick={() => {
                  setChannelEditor({
                    channel: null,
                    parentId: ctxMenu.channelId,
                    tempOnly: canOnlyCreateTemp(ctxChannel),
                  });
                  setCtxMenu(null);
                }}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <line x1="12" y1="5" x2="12" y2="19" />
                  <line x1="5" y1="12" x2="19" y2="12" />
                </svg>
                Create Sub-channel
              </button>
            )}

            {showDelete && (
              <button
                className={`${styles.contextMenuItem} ${styles.contextMenuItemDanger}`}
                onClick={() => {
                  setDeleteConfirm({
                    channelId: ctxMenu.channelId,
                    channelName: ctxChannel?.name ?? "this channel",
                  });
                  setCtxMenu(null);
                }}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="3 6 5 6 21 6" />
                  <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
                  <path d="M10 11v6" />
                  <path d="M14 11v6" />
                  <path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
                </svg>
                Delete Channel
              </button>
            )}
          </div>,
          document.body,
        );
      })()}

      {/* User context menu */}
      {userCtxMenu && (
        <UserContextMenu
          menu={userCtxMenu}
          onClose={() => setUserCtxMenu(null)}
        />
      )}

      {/* Group creation modal */}
      {showGroupModal && (
        <GroupCreateModal
          users={users}
          ownSession={ownSession}
          onClose={() => setShowGroupModal(false)}
          onCreate={async (name, members) => {
            await createGroup(name, members);
            setShowGroupModal(false);
          }}
        />
      )}

      {/* Channel editor dialog */}
      {channelEditor && (
        <ChannelEditorDialog
          channel={channelEditor.channel}
          parentId={channelEditor.parentId}
          tempOnly={channelEditor.tempOnly}
          onClose={() => setChannelEditor(null)}
        />
      )}

      {/* Delete channel confirmation dialog */}
      {deleteConfirm && createPortal(
        <div className={styles.modalOverlay} onClick={() => setDeleteConfirm(null)}>
          <div className={styles.deleteConfirmDialog} onClick={(e) => e.stopPropagation()}>
            <h3 className={styles.deleteConfirmTitle}>Delete Channel</h3>
            <p className={styles.deleteConfirmBody}>
              Are you sure you want to delete <strong>{deleteConfirm.channelName}</strong>?
              This will permanently remove the channel and all its persistent chat messages from the server.
            </p>
            <div className={styles.deleteConfirmActions}>
              <button
                className={styles.deleteConfirmCancel}
                onClick={() => setDeleteConfirm(null)}
              >
                Cancel
              </button>
              <button
                className={styles.deleteConfirmOk}
                onClick={async () => {
                  const id = deleteConfirm.channelId;
                  setDeleteConfirm(null);
                  await deleteChannel(id);
                }}
              >
                Delete
              </button>
            </div>
          </div>
        </div>,
        document.body,
      )}
    </aside>
  );
}
