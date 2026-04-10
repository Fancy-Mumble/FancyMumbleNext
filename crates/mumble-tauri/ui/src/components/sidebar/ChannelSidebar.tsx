import { useState, useMemo, useEffect, useCallback, useRef } from "react";
import { createPortal } from "react-dom";
import { useNavigate } from "react-router-dom";
import { useAppStore } from "../../store";
import type { ChannelEntry, UserEntry, SidebarSections } from "../../types";
import { getPreferences, updatePreferences } from "../../preferencesStorage";
import { SidebarSearchView } from "./SidebarSearchView";
import { UserListItem, colorFor, avatarUrl } from "./UserListItem";
import { UserContextMenu } from "./UserContextMenu";
import type { UserContextMenuState } from "./UserContextMenu";
import ChannelEditorDialog, { canEditChannel, canCreateChannel, canOnlyCreateTemp, canDeleteChannel } from "./ChannelEditorDialog";
import styles from "./ChannelSidebar.module.css";
import CheckIcon from "../../assets/icons/status/check.svg?react";
import ListenBadgeIcon from "../../assets/icons/audio/listen-badge.svg?react";
import MenuIcon from "../../assets/icons/navigation/menu.svg?react";
import SearchIcon from "../../assets/icons/action/search.svg?react";
import CloseIcon from "../../assets/icons/action/close.svg?react";
import ChevronRightIcon from "../../assets/icons/navigation/chevron-right.svg?react";
import PlusIcon from "../../assets/icons/action/plus.svg?react";
import UsersGroupIcon from "../../assets/icons/user/users-group.svg?react";
import MicIcon from "../../assets/icons/audio/mic.svg?react";
import MicOffIcon from "../../assets/icons/audio/mic-off.svg?react";
import MicOffSmallIcon from "../../assets/icons/audio/mic-off-small.svg?react";
import HeadphonesOffIcon from "../../assets/icons/audio/headphones-off.svg?react";
import HeadphonesIcon from "../../assets/icons/audio/headphones.svg?react";
import InfoIcon from "../../assets/icons/status/info.svg?react";
import SettingsIcon from "../../assets/icons/general/settings.svg?react";
import ShieldIcon from "../../assets/icons/status/shield.svg?react";
import LogoutIcon from "../../assets/icons/action/logout.svg?react";
import EditIcon from "../../assets/icons/action/edit.svg?react";
import { isMobile } from "../../utils/platform";
import { loadPersonalization } from "../../personalizationStorage";
import type { ChannelViewerStyle } from "../../personalizationStorage";
import ModernChannelList from "./flat/ModernChannelList";
import ChannelIconList from "./modern/ChannelIconList";
import ClassicChannelList from "./classic/ClassicChannelList";
import TrashIcon from "../../assets/icons/action/trash.svg?react";
import BellIcon from "../../assets/icons/status/bell.svg?react";
import BellOffIcon from "../../assets/icons/status/bell-off.svg?react";
import PhoneIcon from "../../assets/icons/communication/phone.svg?react";
import PhoneOffIcon from "../../assets/icons/communication/phone-off.svg?react";
import RecordIcon from "../../assets/icons/audio/record.svg?react";
import RecordingModal from "./RecordingModal";

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
                    <CheckIcon width={12} height={12} />
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

// --- Self voice controls (extracted for cognitive complexity) ------

interface SelfVoiceControlsProps {
  readonly voiceState: string;
  readonly inCall: boolean;
  readonly toggleMute: () => void;
  readonly toggleDeafen: () => void;
  readonly enableVoice: () => void;
  readonly disableVoice: () => void;
  readonly onCollapse?: () => void;
}

function SelfVoiceControls({ voiceState, inCall, toggleMute, toggleDeafen, enableVoice, disableVoice, onCollapse }: Readonly<SelfVoiceControlsProps>) {
  const isActive = voiceState === "active";
  const isInactive = voiceState === "inactive";
  const muteTitle = isActive ? "Mute" : "Unmute";

  return (<>
    {/* Desktop: mute + deaf toggles (hidden on mobile via CSS) */}
    <div className={`${styles.selfVoiceActions} ${styles.desktopOnly}`}>
      <button
        className={`${styles.voiceToggle} ${isActive ? styles.voiceActive : ""}`}
        onClick={toggleMute}
        title={muteTitle}
      >
        {isActive ? (
          <MicIcon width={18} height={18} />
        ) : (
          <MicOffIcon width={18} height={18} />
        )}
      </button>
      <button
        className={`${styles.voiceToggle} ${isInactive ? "" : styles.voiceActive}`}
        onClick={toggleDeafen}
        title={isInactive ? "Enable Voice" : "Disable Voice"}
      >
        {isInactive ? (
          <HeadphonesOffIcon width={18} height={18} />
        ) : (
          <HeadphonesIcon width={18} height={18} />
        )}
      </button>
    </div>
    {/* Mobile: single call / hang-up button (hidden on desktop via CSS) */}
    <div className={`${styles.selfVoiceActions} ${styles.mobileOnly}`}>
      {inCall ? (
        <button
          className={`${styles.voiceToggle} ${styles.callBtnEnd}`}
          onClick={() => { disableVoice(); onCollapse?.(); }}
          title="End call"
        >
          <PhoneOffIcon width={18} height={18} />
        </button>
      ) : (
        <button
          className={`${styles.voiceToggle} ${styles.callBtnStart}`}
          onClick={() => { enableVoice(); onCollapse?.(); }}
          title="Start call"
        >
          <PhoneIcon width={18} height={18} />
        </button>
      )}
    </div>
  </>);
}

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
  const enableVoice = useAppStore((s) => s.enableVoice);
  const disableVoice = useAppStore((s) => s.disableVoice);
  const inCall = useAppStore((s) => s.inCall);
  const toggleMutePushChannel = useAppStore((s) => s.toggleMutePushChannel);
  const mutedPushChannels = useAppStore((s) => s.mutedPushChannels);
  const navigate = useNavigate();

  // Group chat state
  const groupChats = useAppStore((s) => s.groupChats);
  const selectedGroup = useAppStore((s) => s.selectedGroup);
  const selectGroup = useAppStore((s) => s.selectGroup);
  const groupUnreadCounts = useAppStore((s) => s.groupUnreadCounts);
  const createGroup = useAppStore((s) => s.createGroup);
  const ownSession = useAppStore((s) => s.ownSession);
  const talkingSessions = useAppStore((s) => s.talkingSessions);
  const broadcastingSessions = useAppStore((s) => s.broadcastingSessions);

  const selectDmUser = useAppStore((s) => s.selectDmUser);
  const selectUser = useAppStore((s) => s.selectUser);
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);

  const [channelViewerStyle, setChannelViewerStyle] = useState<ChannelViewerStyle>("flat");
  const [showGroupModal, setShowGroupModal] = useState(false);
  const [showSearch, setShowSearch] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const searchInputRef = useRef<HTMLInputElement>(null);

  // Developer mode: show recording button.
  const [devMode, setDevMode] = useState(false);
  const [showRecordingModal, setShowRecordingModal] = useState(false);
  useEffect(() => {
    getPreferences().then((prefs) => setDevMode(prefs.userMode === "developer"));
  }, []);

  // Load channel viewer style preference.
  useEffect(() => {
    loadPersonalization().then((p) => setChannelViewerStyle(p.channelViewerStyle ?? "flat"));
  }, []);

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

  /** Get the channel name for a user's current channel. */
  const channelName = (channelId: number) => {
    const ch = channels.find((c) => c.id === channelId);
    return ch?.name || "Root";
  };

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
            <MenuIcon width={18} height={18} />
          </button>
        )}
        <div className={styles.searchBar}>
          <SearchIcon className={styles.searchBarIcon} width={14} height={14} />
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
              <CloseIcon width={14} height={14} />
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

      {/* Channel list header (always visible) */}
      <div className={styles.sectionHeaderBar}>
        <button
          className={styles.collapsibleHeader}
          onClick={() => toggleSection("channels", channelsOpen, setChannelsOpen)}
          type="button"
        >
          <ChevronRightIcon
            className={`${styles.collapseChevron} ${channelsOpen ? styles.collapseChevronOpen : ""}`}
            width={12}
            height={12}
          />
          <span>Channels</span>
        </button>
      </div>

      {/* Channel list */}
      <div className={`${styles.channelList} ${channelsOpen ? "" : styles.sectionCollapsed}`}>

        {channelsOpen && channelViewerStyle === "flat" && (
          <ModernChannelList
            channels={channels}
            users={users}
            selectedChannel={selectedChannel}
            currentChannel={currentChannel}
            listenedChannels={listenedChannels}
            unreadCounts={unreadCounts}
            talkingSessions={talkingSessions}
            broadcastingSessions={broadcastingSessions}
            onSelectChannel={(id) => { selectChannel(id); onChannelSelect?.(); }}
            onJoinChannel={(id) => { joinChannel(id); selectChannel(id); onChannelSelect?.(); }}
            onContextMenu={openCtxMenu}
            onUserContextMenu={openUserCtxMenu}
            onUserClick={(session) => { selectDmUser(session); onChannelSelect?.(); }}
          />
        )}

        {channelsOpen && channelViewerStyle === "modern" && (
          <ChannelIconList
            channels={channels}
            users={users}
            selectedChannel={selectedChannel}
            currentChannel={currentChannel}
            listenedChannels={listenedChannels}
            unreadCounts={unreadCounts}
            talkingSessions={talkingSessions}
            broadcastingSessions={broadcastingSessions}
            onSelectChannel={(id) => { selectChannel(id); onChannelSelect?.(); }}
            onJoinChannel={(id) => { joinChannel(id); selectChannel(id); onChannelSelect?.(); }}
            onContextMenu={openCtxMenu}
            onUserContextMenu={openUserCtxMenu}
            onUserClick={(session) => { selectDmUser(session); onChannelSelect?.(); }}
          />
        )}

        {channelsOpen && channelViewerStyle === "classic" && (
          <ClassicChannelList
            channels={channels}
            users={users}
            selectedChannel={selectedChannel}
            currentChannel={currentChannel}
            listenedChannels={listenedChannels}
            unreadCounts={unreadCounts}
            onSelectChannel={(id) => { selectChannel(id); onChannelSelect?.(); }}
            onJoinChannel={(id) => { joinChannel(id); selectChannel(id); onChannelSelect?.(); }}
            onContextMenu={openCtxMenu}
          />
        )}
      </div>

      {!isMobile && <>
      <div className={styles.divider} />

      {/* Group chats */}
      <div className={`${styles.userSection} ${groupsOpen ? "" : styles.sectionCollapsed}`}>
        <div className={styles.groupSectionHeader}>
          <button
            className={styles.collapsibleHeader}
            onClick={() => toggleSection("groups", groupsOpen, setGroupsOpen)}
            type="button"
          >
          <ChevronRightIcon
              className={`${styles.collapseChevron} ${groupsOpen ? styles.collapseChevronOpen : ""}`}
              width={12}
              height={12}
            />
            <span>
              Group Chats{groupChats.length > 0 ? ` - ${groupChats.length}` : ""}
            </span>
          </button>
          <button
            className={styles.newGroupBtn}
            onClick={() => setShowGroupModal(true)}
            title="New group chat"
          >
            <PlusIcon width={14} height={14} />
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
                    <UsersGroupIcon width={16} height={16} />
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
      </>}

      <div className={styles.divider} />

      {/* Online users */}
      <div className={`${styles.userSection} ${onlineOpen ? "" : styles.sectionCollapsed}`}>
        <button
          className={styles.collapsibleHeader}
          onClick={() => toggleSection("online", onlineOpen, setOnlineOpen)}
          type="button"
        >
          <ChevronRightIcon
            className={`${styles.collapseChevron} ${onlineOpen ? styles.collapseChevronOpen : ""}`}
            width={12}
            height={12}
          />
          <span>Online - {users.length}</span>
        </button>
        {onlineOpen &&
          <div className={styles.userList}>
            {users
              .filter((u) => u.session !== ownSession)
              .sort((a, b) => {
                const aInChannel = currentChannel != null && a.channel_id === currentChannel ? 0 : 1;
                const bInChannel = currentChannel != null && b.channel_id === currentChannel ? 0 : 1;
                return aInChannel - bInChannel;
              })
              .map((user) => (
              <UserListItem
                key={user.session}
                user={user}
                channelName={channelName(user.channel_id)}
                active={selectedDmUser === user.session}
                isTalking={talkingSessions.has(user.session)}
                onClick={() => selectDmUser(user.session)}
                onContextMenu={(e) => openUserCtxMenu(e, user)}
              />
            ))}
          </div>
        }
      </div>

      </>)}{/* end search-mode ternary */}

      {/* Self user section - always visible */}
      {(() => {
        const self = users.find((u) => u.session === ownSession);
        if (!self) return null;
        const selfTalking = talkingSessions.has(self.session);
        return (
          <div className={styles.selfUserSection}>
            <UserListItem
              user={self}
              channelName={channelName(self.channel_id)}
              isSelf
              isTalking={selfTalking}
              onClick={() => selectUser(self.session)}
              onContextMenu={(e) => openUserCtxMenu(e, self)}
            />
            {currentChannel != null && (
              <SelfVoiceControls
                voiceState={voiceState}
                inCall={inCall}
                toggleMute={toggleMute}
                toggleDeafen={toggleDeafen}
                enableVoice={enableVoice}
                disableVoice={disableVoice}
                onCollapse={onCollapse}
              />
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
              <InfoIcon width={18} height={18} />
            </button>
          )}
          <button
            className={styles.settingsBtn}
            onClick={() => navigate("/settings")}
            title="Audio settings"
          >
            <SettingsIcon width={18} height={18} />
          </button>
          {isAdmin && (
            <button
              className={styles.adminBtn}
              onClick={() => navigate("/admin")}
              title="Admin panel"
              aria-label="Admin panel"
            >
              <ShieldIcon width={18} height={18} />
            </button>
          )}
          {devMode && voiceState !== "inactive" && (
            <button
              className={`${styles.settingsBtn} ${showRecordingModal ? styles.activeBtn : ""}`}
              onClick={() => setShowRecordingModal(true)}
              title="Record audio"
              aria-label="Record audio"
            >
              <RecordIcon width={18} height={18} />
            </button>
          )}
          <button
            className={styles.disconnectBtn}
            onClick={disconnect}
            title="Disconnect"
          >
            <LogoutIcon width={16} height={16} />
            Disconnect
          </button>
        </div>
      </div>

      {/* Context menu */}
      {ctxMenu && (() => {
        const ctxChannel = channels.find((c) => c.id === ctxMenu.channelId);
        const hasListenPerm = canListen(ctxChannel);
        const isListened = listenedChannels.has(ctxMenu.channelId);
        const isPushMuted = mutedPushChannels.has(ctxMenu.channelId);
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
                  <MicOffSmallIcon width={14} height={14} />
                  Stop listening
                </>
              ) : (
                <>
                  <ListenBadgeIcon width={14} height={14} opacity={hasListenPerm ? 1 : 0.4} />
                  Listen to channel
                </>
              )}
            </button>

            <button
              className={styles.contextMenuItem}
              onClick={() => {
                toggleMutePushChannel(ctxMenu.channelId);
                setCtxMenu(null);
              }}
            >
              {isPushMuted ? (
                <>
                  <BellIcon width={14} height={14} />
                  Enable notifications
                </>
              ) : (
                <>
                  <BellOffIcon width={14} height={14} />
                  Mute notifications
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
                <EditIcon width={14} height={14} />
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
                <PlusIcon width={14} height={14} />
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
                <TrashIcon width={14} height={14} />
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
        <div
          className={styles.modalOverlay}
          role="presentation"
          onClick={() => setDeleteConfirm(null)}
          onKeyDown={(e) => { if (e.key === "Escape") setDeleteConfirm(null); }}
        >
          <div className={styles.deleteConfirmDialog} role="dialog" aria-modal="true" onClick={(e) => e.stopPropagation()}>
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

      {/* Recording modal (developer mode) */}
      {showRecordingModal && (
        <RecordingModal onClose={() => setShowRecordingModal(false)} />
      )}
    </aside>
  );
}
