import { ChevronDownIcon } from "../../icons";
import React, { lazy, Suspense, useState, useEffect, useCallback, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../../store";
import type { ChatMessage, TimeFormat } from "../../types";
import { getPreferences } from "../../preferencesStorage";
import { loadPersonalization, type PersonalizationData } from "../../personalizationStorage";
import ChatHeader from "./ChatHeader";
import type { BroadcastInfo } from "./ChatHeader";
import MobileCallControls from "./MobileCallControls";
const PinnedMessagesPanel = lazy(() => import("./PinnedMessagesPanel"));
const DownloadsPanel = lazy(() => import("./DownloadsPanel"));
import UploadProgressItem, { type UploadPlaceholder } from "./UploadProgressItem";
import PendingMessageItem from "./PendingMessageItem";
import ChatComposer from "./ChatComposer";
import { usePolls } from "./usePolls";
import { useReactions } from "./useReactions";
const MessageContextMenu = lazy(() => import("./MessageContextMenu"));
const MobileMessageActionSheet = lazy(() => import("./MobileMessageActionSheet"));
import MessageSelectionBar from "./MessageSelectionBar";
import ConfirmDialog from "../elements/ConfirmDialog";
import Toast from "../elements/Toast";
import type { FileShareChoice } from "./FileShareDialog";
const FileShareDialog = lazy(() => import("./FileShareDialog"));
import { encodeFileAttachmentMarker, decodeFileAttachmentPayload, previewKindForFilename, FANCY_FILE_MARKER_RE, type FileAttachmentInfo } from "./FileAttachmentCard";
import { usePersistentChat } from "../security/PersistentChatOverlays";
import { BannerStack } from "../security/InfoBanner";
import { useUserAvatars } from "../../lazyBlobs";
import ChatMessageList from "./ChatMessageList";
import QuotePreviewStrip from "./QuotePreviewStrip";
import MentionPopover from "./MentionPopover";
import { useChatSend } from "./useChatSend";
import { useChatScroll } from "./useChatScroll";
import { useMessageSelection } from "./useMessageSelection";
import { useReadReceipts } from "./useReadReceipts";
import { useTypingIndicator } from "./useTypingIndicator";
import TypingIndicator from "./TypingIndicator";
import { isMobile } from "../../utils/platform";
import { htmlToMarkdown } from "./MarkdownInput";
import type { MessageScope } from "../../messageOffload";
import { useScreenShare } from "./useScreenShare";
import ScreenShareViewer, { BroadcastBanner, WebRtcErrorBanner } from "./ScreenShareViewer";
import ActiveWatchBanner from "./watch/ActiveWatchBanner";
import styles from "./ChatView.module.css";
import { Lightbox, type LightboxHandle } from "../elements/Lightbox";

const PollCreator = lazy(() => import("./PollCreator"));
const EmojiPicker = lazy(() => import("../elements/EmojiPicker"));
const StreamFocusView = lazy(() => import("./StreamFocusView"));
const MultiStreamGrid = lazy(() => import("./MultiStreamGrid"));

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
  isDmMode: boolean,
  dmPartner: { name: string } | undefined,
  channel: { name: string } | undefined,
  memberCount: number,
): [string, number] {
  if (isDmMode) return [dmPartner?.name ?? "Direct Message", 0];
  return [channel?.name ?? "Unknown", memberCount];
}

/** Find the first poppable image source in a message body, or null if none. */
function findPopOutImageSrc(body: string): string | null {
  const inline = /<img[^>]+src="([^"]+)"/i.exec(body);
  if (inline?.[1]) return inline[1];
  const fileMatch = FANCY_FILE_MARKER_RE.exec(body);
  if (fileMatch) {
    const info: FileAttachmentInfo | null = decodeFileAttachmentPayload(fileMatch[1]);
    if (info && previewKindForFilename(info.filename) === "image" && info.mode === "public") {
      return info.url;
    }
  }
  return null;
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
  const sfuAvailable = useAppStore((s) => s.serverConfig.webrtc_sfu_available);
  const webrtcError = useAppStore((s) => s.webrtcError);
  const pinMessage = useAppStore((s) => s.pinMessage);
  const clearUnseenPins = useAppStore((s) => s.clearUnseenPins);
  const unseenPinIds = useAppStore((s) => s.unseenPinIds);
  const clearWebRtcError = useCallback(() => useAppStore.setState({ webrtcError: null }), []);

  // DM state
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);
  const dmMessages = useAppStore((s) => s.dmMessages);
  const pendingMessages = useAppStore((s) => s.pendingMessages);

  const isDmMode = selectedDmUser !== null;
  const dmPartner = isDmMode ? users.find((u) => u.session === selectedDmUser) : undefined;

  const [draft, setDraft] = useState("");
  const [pendingQuotes, setPendingQuotes] = useState<ChatMessage[]>([]);
  const [editingMessage, setEditingMessage] = useState<ChatMessage | null>(null);
  const [showPinnedPanel, setShowPinnedPanel] = useState(false);
  const [showDownloadsPanel, setShowDownloadsPanel] = useState(false);
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
    theme: "dark",
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
    if (isDmMode && selectedDmUser !== null) return { scope: "dm", scopeId: String(selectedDmUser) };
    if (selectedChannel !== null) return { scope: "channel", scopeId: String(selectedChannel) };
    return null;
  }, [isDmMode, selectedDmUser, selectedChannel]);

  const channel = channels.find((c) => c.id === selectedChannel);
  const memberCount = users.filter(
    (u) => u.channel_id === selectedChannel,
  ).length;
  const isInChannel = currentChannel === selectedChannel;

  /** Map session -> avatar data-URL for message avatars (lazy-fetched). */
  const avatarBySession = useUserAvatars(users);

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
    isDmMode ? null : selectedChannel,
    channel?.name ?? "Unknown",
  );

  /** Merge real messages with local-only poll messages for rendering. */
  const allMessages = useMemo(() => {
    if (isDmMode) {
      return dmMessages;
    }
    const channelPolls = pollMessages.filter(
      (m) => m.channel_id === selectedChannel,
    );
    return [...messages, ...channelPolls];
  }, [isDmMode, dmMessages, messages, pollMessages, selectedChannel]);

  /** Pending optimistic messages scoped to the current channel / DM. */
  const scopedPending = useMemo(() => {
    if (isDmMode) {
      return pendingMessages.filter((p) => p.dmSession === selectedDmUser);
    }
    return pendingMessages.filter((p) => p.channelId === selectedChannel);
  }, [pendingMessages, isDmMode, selectedDmUser, selectedChannel]);

  const hasNewPins = selectedChannel !== null
    && (unseenPinIds.get(selectedChannel)?.size ?? 0) > 0;

  const channelUnseenPinSet = useMemo(
    () => (selectedChannel !== null
      ? unseenPinIds.get(selectedChannel) ?? new Set<string>()
      : new Set<string>()),
    [unseenPinIds, selectedChannel],
  );

  // Ordered message IDs for read-receipt watermark comparison.
  const allMessageIds = useMemo(
    () => allMessages.map((m) => m.message_id).filter((id): id is string => id != null),
    [allMessages],
  );

  // Auto-send read receipts and query on channel switch.
  const lastMessageId = allMessageIds[allMessageIds.length - 1];
  useReadReceipts(
    isDmMode ? null : selectedChannel,
    lastMessageId,
  );

  // Send typing indicators with debouncing.
  const { notifyTyping, resetTyping } = useTypingIndicator();

  // --- Extracted hooks ---------------------------------------------

  const {
    messagesContainerRef, bottomSentinelRef, messagesInnerRef,
    newMsgCount, lastReadIdx, restoringKeys, handleScrollToBottom,
  } = useChatScroll({ allMessages, selectedChannel, selectedDmUser, currentScope });

  const lightboxRef = useRef<LightboxHandle>(null);

  const handleEdit = useCallback((msg: ChatMessage) => {
    setEditingMessage(msg);
    setDraft(htmlToMarkdown(msg.body));
  }, []);

  const handlePin = useCallback((msg: ChatMessage) => {
    if (!msg.message_id) return;
    const channelId = msg.channel_id ?? selectedChannel ?? 0;
    pinMessage(channelId, msg.message_id, !!msg.pinned);
  }, [selectedChannel, pinMessage]);

  const handlePopOutImage = useCallback((msg: ChatMessage, src: string) => {
    const captionRaw = msg.body
      .replaceAll(/<!--[\s\S]*?-->/g, "")
      .replaceAll(/<img\b[^>]*>/gi, "")
      .replaceAll(/<br\s*\/?>/gi, "\n")
      .replaceAll(/<[^>]*>/g, "")
      .replaceAll("&lt;", "<")
      .replaceAll("&gt;", ">")
      .replaceAll("&amp;", "&")
      .trim();
    const caption = captionRaw.length > 0 ? captionRaw.slice(0, 280) : null;
    const senderAvatar = msg.sender_hash ? avatarByHash.get(msg.sender_hash) ?? null : null;
    const payload = {
      src,
      sender_name: msg.sender_name || null,
      sender_avatar: senderAvatar,
      caption,
      timestamp_ms: msg.timestamp ?? null,
    };
    invoke("open_image_popout", { payload }).catch((err) => {
      console.error("Failed to open image popout:", err);
    });
  }, [avatarByHash]);

  const handleOpenPinnedPanel = useCallback(() => {
    setShowPinnedPanel(true);
    if (selectedChannel !== null) clearUnseenPins(selectedChannel);
  }, [selectedChannel, clearUnseenPins]);

  const handleClosePinnedPanel = useCallback(() => {
    setShowPinnedPanel(false);
  }, []);

  const markDownloadsSeen = useAppStore((s) => s.markDownloadsSeen);
  const unseenDownloadCount = useAppStore((s) => s.unseenDownloadCount);
  const handleOpenDownloadsPanel = useCallback(() => {
    setShowDownloadsPanel(true);
    markDownloadsSeen();
  }, [markDownloadsSeen]);
  const handleCloseDownloadsPanel = useCallback(() => {
    setShowDownloadsPanel(false);
  }, []);

  const cancelEdit = useCallback(() => {
    setEditingMessage(null);
    setDraft("");
  }, []);

  const handleDraftChange = useCallback((value: string) => {
    setDraft(value);
    if (value) notifyTyping();
  }, [notifyTyping]);

  useEffect(() => {
    setEditingMessage(null);
    setShowPinnedPanel(false);
    setUploadPlaceholders([]);
  }, [selectedChannel, selectedDmUser]);

  const {
    canDelete, selectionMode, selectedMsgIds,
    msgContextMenu, deleteConfirm, isDeleting, toast,
    toggleMsgSelection, enterSelectionMode, exitSelectionMode,
    handleMessageContextMenu, handleSingleDelete, handleBulkDelete, confirmDelete,
    handleTouchStart, cancelLongPress,
    handleCite, handleCopyText,
    handleScrollToMessage, removePendingQuote,
    closeContextMenu, clearDeleteConfirm, clearToast, showToast,
  } = useMessageSelection({
    selectedChannel, selectedDmUser,
    channel, messagesContainerRef, setPendingQuotes,
  });

  const { sending, handleSend, sendMediaFile, handlePaste, handleGifSelect } = useChatSend({
    pendingQuotes,
    clearQuotes: () => setPendingQuotes([]),
    draft,
    clearDraft: () => setDraft(""),
    editingMessage,
    onEditComplete: cancelEdit,
    showToast,
  });

  const handleSendAndResetTyping = useCallback(async () => {
    await handleSend();
    resetTyping();
  }, [handleSend, resetTyping]);

  const fileServerConfig = useAppStore((s) => s.fileServerConfig);
  const uploadFile = useAppStore((s) => s.uploadFile);
  const sendMessageAction = useAppStore((s) => s.sendMessage);
  const sendDmAction = useAppStore((s) => s.sendDm);
  const [isUploading, setIsUploading] = useState(false);
  const [shareDialog, setShareDialog] = useState<{ filePath: string; filename: string } | null>(null);
  const [uploadPlaceholders, setUploadPlaceholders] = useState<UploadPlaceholder[]>([]);


  const handleAttachFile = useCallback(async () => {
    if (selectedChannel === null) return;
    if (!fileServerConfig) {
      showToast({
        message: "File sharing is not enabled on this server.",
        variant: "error",
      });
      return;
    }
    if (!fileServerConfig.canShareFiles) {
      showToast({
        message: "You don't have permission to share files in this channel.",
        variant: "error",
      });
      return;
    }
    if (isUploading) return;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ multiple: false, directory: false });
      if (typeof picked !== "string") return;
      const filename = picked.replaceAll("\\", "/").split("/").pop() ?? "file";
      setShareDialog({ filePath: picked, filename });
    } catch (e) {
      console.error("file picker failed:", e);
      const detail = e instanceof Error ? e.message : String(e);
      showToast({ message: `File picker failed: ${detail}`, variant: "error" });
    }
  }, [fileServerConfig, selectedChannel, isUploading, showToast]);

  const performUpload = useCallback(async (
    filePath: string,
    filename: string,
    choice: FileShareChoice,
  ) => {
    if (selectedChannel === null) return;
    const placeholderId = globalThis.crypto?.randomUUID?.() ?? `upload-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    setUploadPlaceholders((prev) => [...prev, { id: placeholderId, filename, state: "uploading" }]);
    // Scroll to show the placeholder after React re-renders.
    requestAnimationFrame(() => {
      const el = messagesContainerRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    });
    setIsUploading(true);
    let unlisten: (() => void) | undefined;
    try {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<{ uploadId: string; bytesSent: number; totalBytes: number }>(
        "upload-progress",
        (event) => {
          if (event.payload.uploadId !== placeholderId) return;
          // Cap at 99: the stream is fully consumed but the server is still
          // processing/responding. We never show 100% until the placeholder is
          // removed on success, so the user can see "still in progress".
          const pct =
            event.payload.totalBytes > 0
              ? Math.min(99, Math.round((event.payload.bytesSent / event.payload.totalBytes) * 100))
              : 0;
          setUploadPlaceholders((prev) =>
            prev.map((p) => (p.id === placeholderId ? { ...p, progress: pct } : p)),
          );
        },
      );
      const resp = await uploadFile({
        filePath,
        channelId: selectedChannel,
        mode: choice.mode,
        password: choice.password,
        filename,
        uploadId: placeholderId,
      });
      const info: FileAttachmentInfo = {
        url: resp.download_url,
        filename,
        sizeBytes: resp.size_bytes,
        mode: resp.access_mode,
        expiresAt: resp.expires_at,
      };
      const marker = encodeFileAttachmentMarker(info);
      const body = choice.message ? `${choice.message}\n${marker}` : marker;
      if (selectedDmUser !== null) {
        await sendDmAction(selectedDmUser, body);
      } else {
        await sendMessageAction(selectedChannel, body);
      }
      setUploadPlaceholders((prev) => prev.filter((p) => p.id !== placeholderId));
    } catch (e) {
      console.error("file upload failed:", e);
      const detail = e instanceof Error ? e.message : String(e);
      // A cancelled upload is silently discarded — the placeholder is already
      // removed by handleCancelUpload, so there is nothing to show.
      if (detail === "upload cancelled") return;
      setUploadPlaceholders((prev) =>
        prev.map((p) =>
          p.id === placeholderId ? { ...p, state: "error" as const, errorMessage: detail } : p,
        ),
      );
    } finally {
      unlisten?.();
      setIsUploading(false);
    }
  }, [selectedChannel, selectedDmUser, uploadFile, sendMessageAction, sendDmAction, messagesContainerRef]);

  const handleShareDialogSubmit = useCallback((choice: FileShareChoice) => {
    const ctx = shareDialog;
    setShareDialog(null);
    if (ctx) void performUpload(ctx.filePath, ctx.filename, choice);
  }, [shareDialog, performUpload]);

  const handleShareDialogCancel = useCallback(() => setShareDialog(null), []);

  const handleDismissUpload = useCallback((id: string) => {
    setUploadPlaceholders((prev) => prev.filter((p) => p.id !== id));
  }, []);

  const handleCancelUpload = useCallback((id: string) => {
    void import("@tauri-apps/api/core").then(({ invoke }) =>
      invoke("cancel_upload", { uploadId: id }),
    );
    setUploadPlaceholders((prev) => prev.filter((p) => p.id !== id));
  }, []);

  const {
    emojiPicker, handleReaction, handleMoreReactions,
    closeEmojiPicker, handleEmojiSelect,
    getMessageReactions, toggleReaction,
  } = useReactions();

  const screenShare = useScreenShare();

  // Determine which screen share panel to show (own broadcast or watching someone).
  // watchingSession takes priority: a broadcaster can watch another stream.
  const activeScreenShare = screenShare.watchingSession !== null
    ? { session: screenShare.watchingSession, isOwn: false, stream: null }
    : screenShare.isBroadcasting
      ? { session: ownSession!, isOwn: true, stream: screenShare.localStream }
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

  // Show StreamFocusView when watching someone, or broadcasting with others.
  // Using a single instance keeps layout state stable across swap transitions.
  const showFocusView = activeScreenShare !== null && (
    !activeScreenShare.isOwn || channelBroadcasters.length > 0
  );

  // Secondary panels for the unified focus view.
  const focusViewSecondaries = useMemo(() => {
    if (!activeScreenShare) return [];
    const secondaries: { session: number; name: string }[] = [];
    if (!activeScreenShare.isOwn && screenShare.isBroadcasting && ownSession !== null) {
      const ownName = users.find((u) => u.session === ownSession)?.name ?? "You";
      secondaries.push({ session: ownSession, name: `${ownName} (you)` });
    }
    for (const b of channelBroadcasters) {
      if (b.session !== activeScreenShare.session) {
        secondaries.push(b);
      }
    }
    return secondaries;
  }, [activeScreenShare, screenShare.isBroadcasting, ownSession, users, channelBroadcasters]);

  const handleFocusWatch = useCallback((session: number) => {
    if (session === ownSession) {
      screenShare.stopWatching();
    } else {
      screenShare.watchBroadcast(session);
    }
  }, [ownSession, screenShare.stopWatching, screenShare.watchBroadcast]);

  // Compute header values before any early returns (hooks can't be conditional).
  const [headerName, headerMemberCount] = computeHeader(
    isDmMode, dmPartner, channel, memberCount,
  );
  const showJoinButton = !isDmMode && !isInChannel;

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

  // Empty state - no channel or DM selected.
  if (selectedChannel === null && !isDmMode) {
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
          isInChannel={isDmMode || isInChannel}
          isDm={isDmMode}
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
          screenShareDisabledReason={
            screenShare.isBroadcastingFromOtherTab
              ? "You are already sharing your screen from another server. Stop that share first."
              : undefined
          }
          sfuAvailable={sfuAvailable}
          broadcastInfo={broadcastInfo}
          hasNewPins={hasNewPins}
          onPinnedMessages={handleOpenPinnedPanel}
          hasNewDownloads={unseenDownloadCount > 0}
          onDownloads={handleOpenDownloadsPanel}
        />
      )}

      {showPinnedPanel && (
        <Suspense fallback={null}>
          <PinnedMessagesPanel
            messages={allMessages}
            unseenIds={channelUnseenPinSet}
            onClose={handleClosePinnedPanel}
            onNavigate={handleScrollToMessage}
            onUnpin={handlePin}
          />
        </Suspense>
      )}

      {showDownloadsPanel && (
        <Suspense fallback={null}>
          <DownloadsPanel onClose={handleCloseDownloadsPanel} />
        </Suspense>
      )}

      <MobileCallControls />

      {/* Solo own broadcast preview (no other broadcasters) */}
      {activeScreenShare?.isOwn && activeScreenShare.stream && !showFocusView && (
        <ScreenShareViewer
          isOwnBroadcast
          localStream={activeScreenShare.stream}
          channelId={selectedChannel ?? 0}
          ownSession={ownSession ?? 0}
        />
      )}

      {/* Unified focus view: single instance keeps layout stable across swaps */}
      {showFocusView && activeScreenShare && (
        <Suspense fallback={null}>
          <StreamFocusView
            isOwnBroadcast={activeScreenShare.isOwn}
            localStream={activeScreenShare.isOwn ? activeScreenShare.stream : null}
            session={activeScreenShare.isOwn ? undefined : activeScreenShare.session}
            ownBroadcastStream={screenShare.isBroadcasting ? screenShare.localStream : null}
            otherBroadcasters={focusViewSecondaries}
            onWatch={handleFocusWatch}
          />
        </Suspense>
      )}

      {/* Multi-stream grid: shown when 2+ broadcasters and we are not sharing or watching */}
      {!activeScreenShare && channelBroadcasters.length > 1 && (
        <Suspense fallback={null}>
          <MultiStreamGrid
            broadcasters={channelBroadcasters}
            onWatch={screenShare.watchBroadcast}
          />
        </Suspense>
      )}

      {/* Single broadcaster notification banner */}
      {!activeScreenShare && channelBroadcasters.length === 1 && (
        <BroadcastBanner
          broadcasters={channelBroadcasters}
          onWatch={screenShare.watchBroadcast}
        />
      )}

      {/* WebRTC error inline banner - same style as broadcast banner */}
      {webrtcError && (
        <WebRtcErrorBanner message={webrtcError} onDismiss={clearWebRtcError} />
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

          <ActiveWatchBanner />

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
          {uploadPlaceholders.map((p) => (
            <UploadProgressItem key={p.id} placeholder={p} onDismiss={handleDismissUpload} onCancel={handleCancelUpload} />
          ))}
          {scopedPending.map((p) => (
            <PendingMessageItem key={p.pendingId} pending={p} />
          ))}
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

      <div className={styles.composerWrapper}>
        <TypingIndicator channelId={isDmMode ? null : selectedChannel} />

        <ChatComposer
          draft={draft}
          onChange={handleDraftChange}
          onSend={handleSendAndResetTyping}
          onPaste={handlePaste}
          onFileSelected={sendMediaFile}
          onGifSelect={handleGifSelect}
          onAttachFile={fileServerConfig?.canShareFiles ? handleAttachFile : undefined}
          disabled={sending || persistent.sendBlocked}
          hasPendingQuotes={pendingQuotes.length > 0}
          isEditing={editingMessage !== null}
          onCancelEdit={cancelEdit}
        />
      </div>

      {showPollCreator && (
        <Suspense fallback={null}>
          <PollCreator
            onSubmit={handlePollCreate}
            onClose={closePollCreator}
          />
        </Suspense>
      )}

      {/* Persistent chat dialogs (key verification, custodian prompt) */}
      {persistent.dialogs}

      {/* Message context menu (right-click on desktop, bottom sheet on mobile) */}
      {msgContextMenu && !isMobile && (
        <Suspense fallback={null}>
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
            onEdit={handleEdit}
            onPin={handlePin}
            onPopOutImage={handlePopOutImage}
            popOutImageSrc={findPopOutImageSrc(msgContextMenu.message.body)}
            reactions={msgContextMenu.message.message_id ? getMessageReactions(msgContextMenu.message.message_id) : []}
            avatarByHash={avatarByHash}
            allMessageIds={allMessageIds}
            channelId={selectedChannel ?? undefined}
          />
        </Suspense>
      )}
      {msgContextMenu && isMobile && (
        <Suspense fallback={null}>
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
            onEdit={handleEdit}
            onPin={handlePin}
            onPopOutImage={handlePopOutImage}
            popOutImageSrc={findPopOutImageSrc(msgContextMenu.message.body)}
            reactions={msgContextMenu.message.message_id ? getMessageReactions(msgContextMenu.message.message_id) : []}
            allMessageIds={allMessageIds}
            channelId={selectedChannel ?? undefined}
            avatarByHash={avatarByHash}
          />
        </Suspense>
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
          isConfirming={isDeleting}
          onConfirm={confirmDelete}
          onCancel={clearDeleteConfirm}
        />
      )}

      {toast && <Toast {...toast} onDismiss={clearToast} />}

      {shareDialog !== null && (
        <Suspense fallback={null}>
          <FileShareDialog
            open={shareDialog !== null}
            filename={shareDialog?.filename ?? ""}
            canSharePublic={fileServerConfig?.canShareFilesPublic ?? true}
            onSubmit={handleShareDialogSubmit}
            onCancel={handleShareDialogCancel}
          />
        </Suspense>
      )}

      {/* Emoji picker overlay */}
      {emojiPicker && (
        <Suspense fallback={null}>
          <EmojiPicker
            anchorX={emojiPicker.x}
            anchorY={emojiPicker.y}
            onSelect={handleEmojiSelect}
            onClose={closeEmojiPicker}
          />
        </Suspense>
      )}

      <Lightbox
        ref={lightboxRef}
        allMessages={allMessages}
        selectedChannel={selectedChannel}
        selectedDmUser={selectedDmUser}
        currentScope={currentScope}
        timeFormat={timeFormat}
        convertToLocalTime={convertToLocalTime}
        systemUses24h={systemUses24h}
      />
      <MentionPopover />
    </main>
  );
}
