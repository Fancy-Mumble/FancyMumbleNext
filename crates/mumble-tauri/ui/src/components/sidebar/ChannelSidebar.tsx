import { BellIcon, BellOffIcon, ChevronRightIcon, CloseIcon, EditIcon, HeadphonesIcon, HeadphonesOffIcon, InfoIcon, ListenBadgeIcon, LogoutIcon, MenuIcon, MicIcon, MicOffIcon, MicOffSmallIcon, PhoneIcon, PhoneOffIcon, PlusIcon, RecordIcon, SearchIcon, SettingsIcon, ShieldIcon, TrashIcon } from "../../icons";
import { useState, useMemo, useEffect, useCallback, useRef, lazy, Suspense } from "react";
import { createPortal } from "react-dom";
import { useNavigate } from "react-router-dom";
import { useAppStore } from "../../store";
import type { ChannelEntry, UserEntry, SidebarSections } from "../../types";
import { getPreferences, updatePreferences } from "../../preferencesStorage";
const SidebarSearchView = lazy(() => import("./SidebarSearchView").then((m) => ({ default: m.SidebarSearchView })));
import { UserListItem, RoleColorsContext, RoleGroupsContext, buildRoleColorMap, buildRoleGroupsMap } from "./UserListItem";
import { useAclGroups } from "../../hooks/useAclGroups";
import { UserContextMenu } from "./UserContextMenu";
import type { UserContextMenuState } from "./UserContextMenu";
import ChannelEditorDialog, { canEditChannel, canCreateChannel, canOnlyCreateTemp, canDeleteChannel } from "./ChannelEditorDialog";
import styles from "./ChannelSidebar.module.css";
import { loadPersonalization } from "../../personalizationStorage";
import type { ChannelViewerStyle } from "../../personalizationStorage";
const ModernChannelList = lazy(() => import("./flat/ModernChannelList"));
const ChannelIconList = lazy(() => import("./modern/ChannelIconList"));
const ClassicChannelList = lazy(() => import("./classic/ClassicChannelList"));
const MembersTab = lazy(() => import("./MembersTab").then((m) => ({ default: m.MembersTab })));
const RecordingModal = lazy(() => import("./RecordingModal"));
import { PERM_LISTEN, PERM_WRITE } from "../../utils/permissions";

/** Check whether a channel's cached permissions include the Listen bit. */
function canListen(channel: ChannelEntry | undefined): boolean {
  if (!channel) return true; // channel not found - allow optimistically
  if (channel.permissions == null) return true; // not yet queried - allow optimistically
  return (channel.permissions & PERM_LISTEN) !== 0;
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

  const ownSession = useAppStore((s) => s.ownSession);
  const talkingSessions = useAppStore((s) => s.talkingSessions);
  const broadcastingSessions = useAppStore((s) => s.broadcastingSessions);

  const aclGroups = useAclGroups();
  const roleColors = useMemo(() => buildRoleColorMap(aclGroups), [aclGroups]);
  const roleGroups = useMemo(() => buildRoleGroupsMap(aclGroups), [aclGroups]);

  const selectDmUser = useAppStore((s) => s.selectDmUser);
  const selectUser = useAppStore((s) => s.selectUser);
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);

  const [channelViewerStyle, setChannelViewerStyle] = useState<ChannelViewerStyle>("flat");
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

  // Sidebar tab — restored from persisted preferences on mount.
  const [activeTab, setActiveTab] = useState<"channels" | "members">("channels");

  // Load persisted section and tab states on mount.
  useEffect(() => {
    getPreferences().then((prefs) => {
      const s = prefs.sidebarSections;
      if (s) {
        setChannelsOpen(s.channels);
      }
      if (prefs.sidebarActiveTab) {
        setActiveTab(prefs.sidebarActiveTab);
      }
    });
  }, []);

  // Persist section states when they change.
  const toggleSection = useCallback(
    (section: keyof SidebarSections, current: boolean, setter: (v: boolean) => void) => {
      const next = !current;
      setter(next);
      getPreferences().then((prefs) => {
        const sections = prefs.sidebarSections ?? { channels: true };
        updatePreferences({ sidebarSections: { ...sections, [section]: next } });
      });
    },
    [],
  );

  const handleTabChange = useCallback((tab: "channels" | "members") => {
    setActiveTab(tab);
    updatePreferences({ sidebarActiveTab: tab }).catch(() => {});
  }, []);

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
    <RoleColorsContext.Provider value={roleColors}>
    <RoleGroupsContext.Provider value={roleGroups}>
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
        <Suspense fallback={null}>
          <SidebarSearchView
            query={searchQuery}
            channelId={searchChannelId}
            channelName={searchChannelName}
            onSelectChannel={(id) => { selectChannel(id); onChannelSelect?.(); }}
            onSelectUser={(session) => { selectDmUser(session); onChannelSelect?.(); }}
          />
        </Suspense>
      ) : (<>

      {/* Tab bar (Channels | Members) */}
      <div className={styles.tabBar} role="tablist">
        <button
          type="button"
          role="tab"
          aria-selected={activeTab === "channels"}
          className={`${styles.tab} ${activeTab === "channels" ? styles.tabActive : ""}`}
          onClick={() => handleTabChange("channels")}
        >
          Channels
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={activeTab === "members"}
          className={`${styles.tab} ${activeTab === "members" ? styles.tabActive : ""}`}
          onClick={() => handleTabChange("members")}
        >
          Members
        </button>
      </div>

      {activeTab === "channels" && <>
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

        <Suspense fallback={null}>
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
        </Suspense>
      </div>
      </>}

      {activeTab === "members" && (
        <Suspense fallback={null}>
          <MembersTab
            users={users}
            channels={channels}
            ownSession={ownSession}
            selectedDmUser={selectedDmUser}
            talkingSessions={talkingSessions}
            onSelectDm={(session) => { selectDmUser(session); onChannelSelect?.(); }}
            onUserContextMenu={openUserCtxMenu}
          />
        </Suspense>
      )}

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
        <Suspense fallback={null}>
          <RecordingModal onClose={() => setShowRecordingModal(false)} />
        </Suspense>
      )}
    </aside>
    </RoleGroupsContext.Provider>
    </RoleColorsContext.Provider>
  );
}
