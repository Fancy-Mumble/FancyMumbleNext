import React, { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../../store";
import type { ChatMessage, TimeFormat } from "../../types";
import { getPreferences } from "../../preferencesStorage";
import { loadPersonalization, type PersonalizationData } from "../../personalizationStorage";
import ChatHeader from "./ChatHeader";
import type { BroadcastInfo } from "./ChatHeader";
import MobileCallControls from "./MobileCallControls";
import ChatComposer from "./ChatComposer";
import PollCreator from "./PollCreator";
import { usePolls } from "./usePolls";
import { useReactions } from "./useReactions";
import EmojiPicker from "../elements/EmojiPicker";
import MessageContextMenu from "./MessageContextMenu";
import MobileMessageActionSheet from "./MobileMessageActionSheet";
import ChevronDownIcon from "../../assets/icons/navigation/chevron-down.svg?react";
import MessageSelectionBar from "./MessageSelectionBar";
import ConfirmDialog from "../elements/ConfirmDialog";
import Toast from "../elements/Toast";
import { usePersistentChat } from "../security/PersistentChatOverlays";
import { BannerStack } from "../security/InfoBanner";
import { textureToDataUrl } from "../../profileFormat";
import ChatMessageList from "./ChatMessageList";
import QuotePreviewStrip from "./QuotePreviewStrip";
import { useChatSend } from "./useChatSend";
import { useChatScroll } from "./useChatScroll";
import { useMessageSelection } from "./useMessageSelection";
import { isMobile } from "../../utils/platform";
import type { MessageScope } from "../../messageOffload";
import { useScreenShare } from "./useScreenShare";
import ScreenShareViewer, { BroadcastBanner } from "./ScreenShareViewer";
import styles from "./ChatView.module.css";
import { Lightbox, type LightboxHandle } from "../elements/Lightbox";

/**
 * Minimum Fancy Mumble server version required for screen sharing.
 * Encoded as (major << 48) | (minor << 32) | (patch << 16).
 * 0.2.12 = (0 << 48) | (2 << 32) | (12 << 16)
 */
const SCREEN_SHARE_MIN_VERSION = 2 * 2 ** 32 + 12 * 2 ** 16;

interface ChatViewProps {
  readonly onChannelInfoToggle?: () => void;
  readonly onChannelSearch?: () => void;
}

/** Compute chat header label and member count based on the active mode. */
function computeHeader(
  isGroupMode: boolean,
  activeGroup: { name: string; members: number[] } | undefined,
  isDmMode: boolean,
  dmPartner: { name: string } | undefined,
  channel: { name: string } | undefined,
  memberCount: number,
): [string, number] {
  if (isGroupMode) return [activeGroup?.name ?? "Group Chat", activeGroup?.members.length ?? 0];
  if (isDmMode) return [dmPartner?.name ?? "Direct Message", 0];
  return [channel?.name ?? "Unknown", memberCount];
}

/** Map a font family id to a CSS font-family string. */
function fontFamilyCss(id: string): string {
  switch (id) {
    case "monospace": return "'Cascadia Mono', 'Fira Code', 'Consolas', monospace";
    case "serif": return "'Georgia', 'Times New Roman', serif";
    case "humanist": return "'Segoe UI', 'Helvetica Neue', 'Arial', sans-serif";
    case "rounded": return "'Nunito', 'Quicksand', 'Comfortaa', sans-serif";
    default: return "inherit";
  }
}

export default function ChatView({ onChannelInfoToggle, onChannelSearch }: ChatViewProps) {
  const channels = useAppStore((s) => s.channels);
  const users = useAppStore((s) => s.users);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const currentChannel = useAppStore((s) => s.currentChannel);
  const messages = useAppStore((s) => s.messages);
  const joinChannel = useAppStore((s) => s.joinChannel);
  const ownSession = useAppStore((s) => s.ownSession);
  const selectUser = useAppStore((s) => s.selectUser);
  const toggleSilenceChannel = useAppStore((s) => s.toggleSilenceChannel);
  const silencedChannels = useAppStore((s) => s.silencedChannels);
  const serverFancyVersion = useAppStore((s) => s.serverFancyVersion);

  // DM state
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);
  const dmMessages = useAppStore((s) => s.dmMessages);

  // Group chat state
  const selectedGroup = useAppStore((s) => s.selectedGroup);
  const groupMessages = useAppStore((s) => s.groupMessages);
  const groupChats = useAppStore((s) => s.groupChats);

  const isDmMode = selectedDmUser !== null;
  const isGroupMode = selectedGroup !== null;
  const dmPartner = isDmMode ? users.find((u) => u.session === selectedDmUser) : undefined;
  const activeGroup = isGroupMode ? groupChats.find((g) => g.id === selectedGroup) : undefined;

  const [draft, setDraft] = useState("");
  const [pendingQuotes, setPendingQuotes] = useState<ChatMessage[]>([]);
  const {
    polls, pollMessages, showPollCreator, openPollCreator, closePollCreator,
    handlePollCreate, handlePollVote,
  } = usePolls();

  // Time display preferences (loaded once from persistent storage).
  const [timeFormat, setTimeFormat] = useState<TimeFormat>("auto");
  const [convertToLocalTime, setConvertToLocalTime] = useState(true);
  const [systemUses24h, setSystemUses24h] = useState<boolean | undefined>(undefined);

  const [personalization, setPersonalization] = useState<PersonalizationData>({
    chatBgOriginal: null,
    chatBgBlurred: null,
    chatBgBlurSigma: 0,
    chatBgOpacity: 0.25,
    chatBgDim: 0.5,
    chatBgFit: "cover",
    bubbleStyle: "bubbles",
    fontSize: "medium",
    fontSizeCustomPx: 14,
    fontFamily: "system",
    compactMode: false,
    channelViewerStyle: "modern",
  });

  useEffect(() => {
    getPreferences().then((prefs) => {
      setTimeFormat(prefs.timeFormat);
      setConvertToLocalTime(prefs.convertToLocalTime);
    });
    loadPersonalization().then(setPersonalization).catch(() => { /* keep defaults */ });
    invoke<"12h" | "24h" | null>("get_system_clock_format")
      .then((fmt) => {
        if (fmt !== null) setSystemUses24h(fmt === "24h");
      })
      .catch(() => { /* leave undefined - fall back to Intl */ });
  }, []);

  /** Build the `MessageScope` for the current chat mode. */
  const currentScope = useCallback((): MessageScope | null => {
    if (isGroupMode && selectedGroup) return { scope: "group", scopeId: selectedGroup };
    if (isDmMode && selectedDmUser !== null) return { scope: "dm", scopeId: String(selectedDmUser) };
    if (selectedChannel !== null) return { scope: "channel", scopeId: String(selectedChannel) };
    return null;
  }, [isGroupMode, selectedGroup, isDmMode, selectedDmUser, selectedChannel]);

  const channel = channels.find((c) => c.id === selectedChannel);
  const memberCount = users.filter(
    (u) => u.channel_id === selectedChannel,
  ).length;
  const isInChannel = currentChannel === selectedChannel;

  /** Map session -> avatar data-URL for message avatars (cached). */
  const avatarCache = React.useRef(new Map<number, { len: number; url: string }>());
  const avatarBySession = useMemo(() => {
    const cache = avatarCache.current;
    const map = new Map<number, string>();
    for (const u of users) {
      if (u.texture && u.texture.length > 0) {
        const prev = cache.get(u.session);
        if (prev?.len === u.texture.length) {
          map.set(u.session, prev.url);
        } else {
          const url = textureToDataUrl(u.texture);
          cache.set(u.session, { len: u.texture.length, url });
          map.set(u.session, url);
        }
      }
    }
    return map;
  }, [users]);

  /** Map session -> UserEntry for quick lookup. */
  const userBySession = useMemo(() => {
    const map = new Map<number, (typeof users)[number]>();
    for (const u of users) {
      map.set(u.session, u);
    }
    return map;
  }, [users]);

  /** Map cert-hash -> UserEntry for resolving stored messages after reconnect. */
  const userByHash = useMemo(() => {
    const map = new Map<string, (typeof users)[number]>();
    for (const u of users) {
      if (u.hash) map.set(u.hash, u);
    }
    return map;
  }, [users]);

  /** Map cert-hash -> avatar data-URL for hash-based avatar lookup. */
  const avatarByHash = useMemo(() => {
    const map = new Map<string, string>();
    for (const u of users) {
      if (u.hash) {
        const url = avatarBySession.get(u.session);
        if (url) map.set(u.hash, url);
      }
    }
    return map;
  }, [users, avatarBySession]);

  // Persistent chat hook (banners, key verification, custodian prompt).
  const persistent = usePersistentChat(
    isDmMode || isGroupMode ? null : selectedChannel,
    channel?.name ?? "Unknown",
  );

  /** Merge real messages with local-only poll messages for rendering. */
  const allMessages = useMemo(() => {
    if (isGroupMode) {
      return groupMessages;
    }
    if (isDmMode) {
      return dmMessages;
    }
    const channelPolls = pollMessages.filter(
      (m) => m.channel_id === selectedChannel,
    );
    return [...messages, ...channelPolls];
  }, [isGroupMode, groupMessages, isDmMode, dmMessages, messages, pollMessages, selectedChannel]);

  // --- Extracted hooks ---------------------------------------------

  const {
    messagesContainerRef, bottomSentinelRef, messagesInnerRef,
    newMsgCount, lastReadIdx, restoringKeys, handleScrollToBottom,
  } = useChatScroll({ allMessages, selectedChannel, selectedDmUser, selectedGroup, currentScope });

  const lightboxRef = useRef<LightboxHandle>(null);

  const { sending, handleSend, sendMediaFile, handlePaste, handleGifSelect } = useChatSend({
    pendingQuotes,
    clearQuotes: () => setPendingQuotes([]),
    draft,
    clearDraft: () => setDraft(""),
  });

  const {
    canDelete, selectionMode, selectedMsgIds,
    msgContextMenu, deleteConfirm, toast,
    toggleMsgSelection, enterSelectionMode, exitSelectionMode,
    handleMessageContextMenu, handleSingleDelete, handleBulkDelete, confirmDelete,
    handleTouchStart, cancelLongPress,
    handleCite, handleCopyText,
    handleScrollToMessage, removePendingQuote,
    closeContextMenu, clearDeleteConfirm, clearToast,
  } = useMessageSelection({
    selectedChannel, selectedDmUser, selectedGroup,
    channel, messagesContainerRef, setPendingQuotes,
  });

  const {
    emojiPicker, handleReaction, handleMoreReactions,
    closeEmojiPicker, handleEmojiSelect,
    getMessageReactions, toggleReaction,
  } = useReactions();

  const screenShare = useScreenShare();

  // Determine which screen share panel to show (own broadcast or watching someone).
  const activeScreenShare = screenShare.isBroadcasting
    ? { session: ownSession!, isOwn: true, stream: screenShare.localStream }
    : screenShare.watchingSession !== null
      ? { session: screenShare.watchingSession, isOwn: false, stream: null }
      : null;

  // Other users broadcasting in the current channel (for the notification banner).
  const channelBroadcasters = useMemo(() => {
    if (screenShare.broadcastingSessions.size === 0) return [];
    return users
      .filter((u) => u.channel_id === selectedChannel
        && screenShare.broadcastingSessions.has(u.session)
        && u.session !== ownSession)
      .map((u) => ({ session: u.session, name: u.name }));
  }, [users, selectedChannel, screenShare.broadcastingSessions, ownSession]);

  // Compute header values before any early returns (hooks can't be conditional).
  const [headerName, headerMemberCount] = computeHeader(
    isGroupMode, activeGroup, isDmMode, dmPartner, channel, memberCount,
  );
  const showJoinButton = !isDmMode && !isGroupMode && !isInChannel;

  // Build broadcastInfo for the header when a stream is active.
  const broadcastInfo = useMemo((): BroadcastInfo | undefined => {
    if (!activeScreenShare) return undefined;
    const broadcaster = users.find((u) => u.session === activeScreenShare.session);
    const name = broadcaster?.name ?? "User";
    const avatar = avatarBySession.get(activeScreenShare.session) ?? null;
    const viewers = broadcaster
      ? users.filter((u) => u.channel_id === broadcaster.channel_id).length - 1
      : users.length - 1;
    return {
      broadcasterName: name,
      avatarUrl: avatar,
      viewerCount: viewers,
      isOwnBroadcast: activeScreenShare.isOwn,
      channelName: channel?.name ?? "Unknown",
      onClose: activeScreenShare.isOwn ? screenShare.stopSharing : screenShare.stopWatching,
    };
  }, [activeScreenShare, users, avatarBySession, screenShare.stopSharing, screenShare.stopWatching]);

  // Empty state - no channel, DM, or group selected.
  if (selectedChannel === null && !isDmMode && !isGroupMode) {
    return (
      <main className={styles.main}>
        <div className={styles.empty}>
          <div className={styles.emptyIcon}>💬</div>
          <p>Select a channel to start chatting</p>
        </div>
      </main>
    );
  }

  return (
    <main className={`${styles.main} ${activeScreenShare ? styles.streamingLayout : ""}`}>
      {selectionMode ? (
        <MessageSelectionBar
          count={selectedMsgIds.size}
          onDelete={handleBulkDelete}
          onCancel={exitSelectionMode}
        />
      ) : (
        <ChatHeader
          channelName={headerName}
          memberCount={headerMemberCount}
          isInChannel={isDmMode || isGroupMode || isInChannel}
          isDm={isDmMode}
          isGroup={isGroupMode}
          isPersisted={persistent.isPersisted}
          onJoin={showJoinButton ? () => joinChannel(selectedChannel!) : undefined}
          onChannelInfoToggle={onChannelInfoToggle}
          onChannelSearch={onChannelSearch}
          keyTrustLevel={persistent.trustLevel}
          onVerifyClick={persistent.onVerifyClick}
          onPollCreate={openPollCreator}
          isSilenced={selectedChannel !== null && silencedChannels.has(selectedChannel)}
          onToggleSilence={selectedChannel !== null ? () => toggleSilenceChannel(selectedChannel) : undefined}
          isScreenSharing={screenShare.isBroadcasting}
          onToggleScreenShare={
            !isMobile && serverFancyVersion != null && serverFancyVersion >= SCREEN_SHARE_MIN_VERSION
              ? (screenShare.isBroadcasting ? screenShare.stopSharing : screenShare.startSharing)
              : undefined
          }
          broadcastInfo={broadcastInfo}
        />
      )}

      <MobileCallControls />

      {/* Screen share viewer panel */}
      {activeScreenShare && (
        <ScreenShareViewer
          isOwnBroadcast={activeScreenShare.isOwn}
          localStream={activeScreenShare.stream}
        />
      )}

      {/* Notification banner when someone in the channel is sharing */}
      {!activeScreenShare && channelBroadcasters.length > 0 && (
        <BroadcastBanner
          broadcasters={channelBroadcasters}
          onWatch={screenShare.watchBroadcast}
        />
      )}

      {/* Messages wrapper: position:relative so the key-share banner
           can overlay the scroll viewport without scrolling with it */}
      <div className={styles.messagesWrapper}>
        {persistent.keyShareBanner && (
          <div className={styles.fixedKeyShareBanner}>
            {persistent.keyShareBanner}
          </div>
        )}

        {/* Messages */}
        <div
          ref={messagesContainerRef}
          className={[
            styles.messages,
            personalization.bubbleStyle === "flat" ? styles.flatStyle : "",
            personalization.bubbleStyle === "compact" ? styles.compactStyle : "",
            personalization.compactMode ? styles.compactLayout : "",
          ].join(" ")}
          data-has-bg={personalization.chatBgOriginal ? "" : undefined}
          style={{
            ...(personalization.chatBgOriginal ? {
              "--chat-bg-image": `url(${personalization.chatBgBlurred ?? personalization.chatBgOriginal})`,
              "--chat-bg-opacity": String(personalization.chatBgOpacity),
              "--chat-bg-size": personalization.chatBgFit === "tile" ? "auto" : "cover",
              "--chat-bg-repeat": personalization.chatBgFit === "tile" ? "repeat" : "no-repeat",
            } : {}),
            "--chat-font-size": personalization.fontSize === "small" ? "12px"
              : personalization.fontSize === "large" ? `${personalization.fontSizeCustomPx}px`
              : "14px",
            "--chat-font-family": fontFamilyCss(personalization.fontFamily),
          } as React.CSSProperties}
        >
          <div ref={messagesInnerRef} className={styles.messagesInner}>
          {/* All banners in a single sticky container */}
          <BannerStack>
            {persistent.banner}
            {persistent.signalBridgeErrorBanner}
            {persistent.disputeBanner}
            {persistent.revokedBanner}
          </BannerStack>

          {allMessages.length === 0 ? (
            <div className={styles.empty}>
              <div className={styles.emptyIcon}>👋</div>
              <p>No messages yet. Say hello!</p>
            </div>
          ) : (
            <ChatMessageList
              allMessages={allMessages}
              userBySession={userBySession}
              avatarBySession={avatarBySession}
              userByHash={userByHash}
              avatarByHash={avatarByHash}
              convertToLocalTime={convertToLocalTime}
              bubbleStyle={personalization.bubbleStyle}
              lastReadIdx={lastReadIdx}
              selectionMode={selectionMode}
              canDelete={canDelete}
              selectedMsgIds={selectedMsgIds}
              restoringKeys={restoringKeys}
              polls={polls}
              ownSession={ownSession}
              timeFormat={timeFormat}
              systemUses24h={systemUses24h}
              selectUser={selectUser}
              handleMessageContextMenu={handleMessageContextMenu}
              toggleMsgSelection={toggleMsgSelection}
              handleCite={handleCite}
              handleTouchStart={handleTouchStart}
              cancelLongPress={cancelLongPress}
              handleReaction={handleReaction}
              handleMoreReactions={handleMoreReactions}
              handleCopyText={handleCopyText}
              handleSingleDelete={handleSingleDelete}
              handlePollVote={handlePollVote}
              handleScrollToMessage={handleScrollToMessage}
              handleOpenLightbox={(src) => lightboxRef.current?.open(src)}
              getMessageReactions={getMessageReactions}
              onToggleReaction={toggleReaction}
              onAddReaction={handleMoreReactions}
            />
          )}
          {/* Bottom sentinel - scroll target for auto-scroll */}
          <div ref={bottomSentinelRef} aria-hidden="true" style={{ height: 0, overflow: "hidden" }} />
        </div>
        </div>
      </div>

      {/* "New messages" pill - shown when user scrolled up and messages arrive */}
      {newMsgCount > 0 && (
        <button
          className={styles.newMessagesPill}
          onClick={handleScrollToBottom}
        >
          <ChevronDownIcon width={16} height={16} aria-hidden="true" />
          {newMsgCount} new {newMsgCount === 1 ? "message" : "messages"}
        </button>
      )}

      {/* Pending quote preview strip */}
      <QuotePreviewStrip quotes={pendingQuotes} onRemove={removePendingQuote} />

      <ChatComposer
        draft={draft}
        onChange={setDraft}
        onSend={handleSend}
        onPaste={handlePaste}
        onFileSelected={sendMediaFile}
        onGifSelect={handleGifSelect}
        disabled={sending || persistent.keyRevoked}
        hasPendingQuotes={pendingQuotes.length > 0}
      />

      {showPollCreator && (
        <PollCreator
          onSubmit={handlePollCreate}
          onClose={closePollCreator}
        />
      )}

      {/* Persistent chat dialogs (key verification, custodian prompt) */}
      {persistent.dialogs}

      {/* Message context menu (right-click on desktop, bottom sheet on mobile) */}
      {msgContextMenu && !isMobile && (
        <MessageContextMenu
          menu={msgContextMenu}
          canDelete={canDelete}
          onClose={closeContextMenu}
          onDelete={handleSingleDelete}
          onSelectMode={enterSelectionMode}
          onReaction={handleReaction}
          onMoreReactions={handleMoreReactions}
          onCite={handleCite}
          onCopyText={handleCopyText}
          reactions={msgContextMenu.message.message_id ? getMessageReactions(msgContextMenu.message.message_id) : []}
          avatarBySession={avatarBySession}
          avatarByHash={avatarByHash}
        />
      )}
      {msgContextMenu && isMobile && (
        <MobileMessageActionSheet
          message={msgContextMenu.message}
          canDelete={canDelete}
          onClose={closeContextMenu}
          onDelete={handleSingleDelete}
          onSelectMode={enterSelectionMode}
          onReaction={handleReaction}
          onMoreReactions={handleMoreReactions}
          onCite={handleCite}
          onCopyText={handleCopyText}
          reactions={msgContextMenu.message.message_id ? getMessageReactions(msgContextMenu.message.message_id) : []}
        />
      )}

      {/* Delete confirmation dialog */}
      {deleteConfirm && (
        <ConfirmDialog
          title="Delete messages"
          body={
            deleteConfirm.ids.length === 1
              ? "Are you sure you want to delete this message? This action cannot be undone."
              : `Are you sure you want to delete ${deleteConfirm.ids.length} messages? This action cannot be undone.`
          }
          confirmLabel="Delete"
          danger
          onConfirm={confirmDelete}
          onCancel={clearDeleteConfirm}
        />
      )}

      {toast && <Toast {...toast} onDismiss={clearToast} />
      }

      {/* Emoji picker overlay */}
      {emojiPicker && (
        <EmojiPicker
          anchorX={emojiPicker.x}
          anchorY={emojiPicker.y}
          onSelect={handleEmojiSelect}
          onClose={closeEmojiPicker}
        />
      )}

      <Lightbox
        ref={lightboxRef}
        allMessages={allMessages}
        selectedChannel={selectedChannel}
        selectedDmUser={selectedDmUser}
        selectedGroup={selectedGroup}
        currentScope={currentScope}
        timeFormat={timeFormat}
        convertToLocalTime={convertToLocalTime}
        systemUses24h={systemUses24h}
      />
    </main>
  );
}
