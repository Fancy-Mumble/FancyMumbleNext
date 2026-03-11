import { useState, useRef, useEffect, useCallback, useMemo, type ClipboardEvent } from "react";
import { useAppStore } from "../store";
import MediaPreview from "./MediaPreview";
import MarkdownInput, { markdownToHtml } from "./MarkdownInput";
import GifPicker from "./GifPicker";
import PollCreator from "./PollCreator";
import type { PollPayload, PollVotePayload } from "./PollCreator";
import PollCard, { registerVote, registerLocalVote, getPoll } from "./PollCard";
import { mediaKind, fileToDataUrl, fitImage, fitVideo, mediaToHtml } from "../utils/media";
import { textureToDataUrl } from "../profileFormat";
import styles from "./ChatView.module.css";

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

export default function ChatView() {
  const channels = useAppStore((s) => s.channels);
  const users = useAppStore((s) => s.users);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const currentChannel = useAppStore((s) => s.currentChannel);
  const messages = useAppStore((s) => s.messages);
  const sendMessage = useAppStore((s) => s.sendMessage);
  const joinChannel = useAppStore((s) => s.joinChannel);
  const serverConfig = useAppStore((s) => s.serverConfig);
  const sendPluginData = useAppStore((s) => s.sendPluginData);
  const ownSession = useAppStore((s) => s.ownSession);
  const addPoll = useAppStore((s) => s.addPoll);
  const polls = useAppStore((s) => s.polls);
  const pollMessages = useAppStore((s) => s.pollMessages);

  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [showGifPicker, setShowGifPicker] = useState(false);
  const [showPollCreator, setShowPollCreator] = useState(false);
  const [_pollRev, forceRender] = useState(0);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Ref to latest users array so async callbacks get current data
  // without requiring effect re-registration.
  const usersRef = useRef(users);
  usersRef.current = users;

  const channel = channels.find((c) => c.id === selectedChannel);
  const memberCount = users.filter(
    (u) => u.channel_id === selectedChannel,
  ).length;
  const isInChannel = currentChannel === selectedChannel;

  /** Map session → avatar data-URL for message avatars (cached). */
  const avatarCache = useRef(new Map<number, { len: number; url: string }>());
  const avatarBySession = useMemo(() => {
    const cache = avatarCache.current;
    const map = new Map<number, string>();
    for (const u of users) {
      if (u.texture && u.texture.length > 0) {
        const prev = cache.get(u.session);
        if (prev && prev.len === u.texture.length) {
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

  /** Merge real messages with local-only poll messages for rendering. */
  const allMessages = useMemo(() => {
    const channelPolls = pollMessages.filter(
      (m) => m.channel_id === selectedChannel,
    );
    return [...messages, ...channelPolls];
  }, [messages, pollMessages, selectedChannel]);

  // Auto-scroll to bottom on new messages.
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [allMessages]);

  const handleSend = async () => {
    const text = draft.trim();
    if (!text || selectedChannel === null) return;
    setDraft("");
    const html = markdownToHtml(text);
    await sendMessage(selectedChannel, html);
  };

  const handleAttach = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  /** Shared logic: encode a File and send it as a media message. */
  const sendMediaFile = useCallback(
    async (file: File) => {
      if (selectedChannel === null) return;

      const kind = mediaKind(file.type);
      if (!kind) {
        alert("Unsupported file type. Please select an image, GIF, or video.");
        return;
      }

      // 0 means "no special image limit" → fall back to message_length.
      const maxBytes =
        serverConfig.max_image_message_length > 0
          ? serverConfig.max_image_message_length
          : serverConfig.max_message_length;

      setSending(true);
      try {
        let dataUrl: string;
        let sendKind = kind;

        if (kind === "image") {
          dataUrl = await fitImage(file, maxBytes);
        } else if (kind === "video") {
          const result = await fitVideo(file, maxBytes);
          dataUrl = result.dataUrl;
          sendKind = result.kind; // may become "image" if poster extracted
        } else {
          // GIF - pass through if it fits, otherwise re-encode as JPEG
          dataUrl = await fileToDataUrl(file);
          if (dataUrl.length > maxBytes) {
            dataUrl = await fitImage(file, maxBytes);
            sendKind = "image";
          }
        }

        const html = mediaToHtml(dataUrl, sendKind, file.name || "clipboard.png");
        await sendMessage(selectedChannel, html);
      } catch (err) {
        console.error("media send error:", err);
        alert(String(err));
      } finally {
        setSending(false);
      }
    },
    [selectedChannel, serverConfig, sendMessage],
  );

  const handleFileSelected = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;
      e.target.value = "";
      await sendMediaFile(file);
    },
    [sendMediaFile],
  );

  /** Handle Ctrl+V / Cmd+V with image data on the clipboard. */
  const handlePaste = useCallback(
    (e: ClipboardEvent) => {
      const items = e.clipboardData?.items;
      if (!items) return;

      for (const item of items) {
        if (item.kind === "file" && item.type.startsWith("image/")) {
          e.preventDefault();
          const file = item.getAsFile();
          if (file) sendMediaFile(file);
          return;
        }
      }
      // If no image found, let the default paste into the text input happen.
    },
    [sendMediaFile],
  );

  // ─── GIF picker handler ─────────────────────────────────────────

  const handleGifSelect = useCallback(
    async (url: string, alt: string) => {
      if (selectedChannel === null) return;
      const html = `<img src="${url}" alt="${alt}" />`;
      await sendMessage(selectedChannel, html);
    },
    [selectedChannel, sendMessage],
  );

  // ─── Poll handlers ─────────────────────────────────────────────

  const handlePollCreate = useCallback(
    async (question: string, options: string[], multiple: boolean) => {
      if (selectedChannel === null) return;

      const currentUsers = usersRef.current;
      const ownUser = currentUsers.find((u) => u.session === ownSession);
      const pollId = crypto.randomUUID();
      const poll: PollPayload = {
        type: "poll",
        id: pollId,
        question,
        options,
        multiple,
        creator: ownSession ?? 0,
        creatorName: ownUser?.name ?? "",
        createdAt: new Date().toISOString(),
        channelId: selectedChannel,
      };

      // Register locally via the Zustand store.
      addPoll(poll, true);

      // The Mumble server only forwards PluginDataTransmission to
      // explicitly listed sessions - an empty list means nobody receives it.
      const targets = currentUsers
        .filter((u) => u.channel_id === selectedChannel && u.session !== ownSession)
        .map((u) => u.session);
      const data = new TextEncoder().encode(JSON.stringify(poll));
      await sendPluginData(targets, data, "fancy-poll");
    },
    [selectedChannel, sendPluginData, ownSession, addPoll],
  );

  const handlePollVote = useCallback(
    async (pollId: string, selected: number[]) => {
      const currentUsers = usersRef.current;
      const ownUser = currentUsers.find((u) => u.session === ownSession);
      const vote: PollVotePayload = {
        type: "poll_vote",
        pollId,
        selected,
        voter: ownSession ?? 0,
        voterName: ownUser?.name ?? "",
      };

      registerVote(vote);
      registerLocalVote(pollId, selected);
      forceRender((n) => n + 1);

      // Look up the poll to determine its channel for targeting.
      const pollData = getPoll(pollId);
      const targetChannel = pollData?.channelId ?? selectedChannel ?? 0;

      // The Mumble server requires explicit receiver sessions.
      const targets = currentUsers
        .filter((u) => u.channel_id === targetChannel && u.session !== ownSession)
        .map((u) => u.session);
      const data = new TextEncoder().encode(JSON.stringify(vote));
      await sendPluginData(targets, data, "fancy-poll-vote");
    },
    [sendPluginData, ownSession, selectedChannel],
  );

  // ─── End poll handlers ───────────────────────────────────────────

  // Empty state - no channel selected.
  if (selectedChannel === null) {
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
    <main className={styles.main}>
      {/* Header */}
      <div className={styles.header}>
        <div className={styles.headerInfo}>
          <h2 className={styles.channelName}>
            # {channel?.name || "Unknown"}
          </h2>
          <span className={styles.memberCount}>{memberCount} members</span>
        </div>
        {!isInChannel && (
          <button
            className={styles.joinBtn}
            onClick={() => joinChannel(selectedChannel)}
          >
            Join Channel
          </button>
        )}
      </div>

      {/* Messages */}
      <div className={styles.messages}>
        {allMessages.length === 0 ? (
          <div className={styles.empty}>
            <div className={styles.emptyIcon}>👋</div>
            <p>No messages yet. Say hello!</p>
          </div>
        ) : (
          allMessages.map((msg, i) => (
            <div
              key={i}
              className={`${styles.messageRow} ${msg.is_own ? styles.own : ""}`}
            >
              {!msg.is_own && (
                (() => {
                  const avUrl = msg.sender_session != null
                    ? avatarBySession.get(msg.sender_session)
                    : undefined;
                  return avUrl ? (
                    <img
                      src={avUrl}
                      alt={msg.sender_name}
                      className={styles.messageAvatarImg}
                    />
                  ) : (
                    <div
                      className={styles.messageAvatar}
                      style={{ background: colorFor(msg.sender_name) }}
                    >
                      {msg.sender_name.charAt(0).toUpperCase()}
                    </div>
                  );
                })()
              )}
              <div
                className={`${styles.bubble} ${msg.is_own ? styles.ownBubble : ""}`}
              >
                {!msg.is_own && (
                  <span
                    className={styles.senderName}
                    style={{ color: colorFor(msg.sender_name) }}
                  >
                    {msg.sender_name}
                  </span>
                )}
                <div className={styles.messageBody}>
                  {(() => {
                    // Check for poll marker: <!-- FANCY_POLL:uuid -->
                    const pollMatch = /<!-- FANCY_POLL:(.+?) -->/.exec(msg.body);
                    if (pollMatch) {
                      const pollId = pollMatch[1];
                      const poll = polls.get(pollId) ?? getPoll(pollId);
                      if (poll) {
                        return (
                          <PollCard
                            poll={poll}
                            ownSession={ownSession}
                            isOwn={msg.is_own}
                            onVote={handlePollVote}
                          />
                        );
                      }
                    }
                    return <MediaPreview html={msg.body} messageId={`${i}`} />;
                  })()}
                </div>
              </div>
            </div>
          ))
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* Composer */}
      <div className={styles.composerWrapper}>
        {showGifPicker && (
          <GifPicker
            onSelect={handleGifSelect}
            onClose={() => setShowGifPicker(false)}
          />
        )}
        {showPollCreator && (
          <PollCreator
            onSubmit={handlePollCreate}
            onClose={() => setShowPollCreator(false)}
          />
        )}
        <div className={styles.composer}>
          <input
            ref={fileInputRef}
            type="file"
            accept="image/*,video/*"
            className={styles.hiddenFileInput}
            onChange={handleFileSelected}
          />
          <button
            className={styles.attachBtn}
            onClick={handleAttach}
            disabled={sending}
            title="Attach image, GIF, or video"
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M21.44 11.05l-9.19 9.19a6 6 0 01-8.49-8.49l9.19-9.19a4 4 0 015.66 5.66l-9.2 9.19a2 2 0 01-2.83-2.83l8.49-8.48" />
            </svg>
          </button>

          {/* GIF button */}
          <button
            className={`${styles.attachBtn} ${showGifPicker ? styles.attachBtnActive : ""}`}
            onClick={() => setShowGifPicker((s) => !s)}
            disabled={sending}
            title="GIF picker"
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <rect x="2" y="2" width="20" height="20" rx="5" />
              <text x="12" y="16" textAnchor="middle" fill="currentColor" stroke="none" fontSize="10" fontWeight="bold">GIF</text>
            </svg>
          </button>

          {/* Poll button */}
          <button
            className={styles.attachBtn}
            onClick={() => setShowPollCreator(true)}
            disabled={sending}
            title="Create a poll"
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" stroke="none">
              <path d="M3 3h4v18H3V3zm7 4h4v14h-4V7zm7 4h4v10h-4V11z" />
            </svg>
          </button>

          <MarkdownInput
            value={draft}
            onChange={setDraft}
            onSubmit={handleSend}
            onPaste={handlePaste}
            placeholder="Write a message… (Ctrl+B/I/U for formatting)"
            disabled={sending}
          />

          <button
            className={styles.sendBtn}
            onClick={handleSend}
            disabled={!draft.trim() || sending}
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
              <path d="M2.01 21L23 12 2.01 3 2 10l15 2-15 2z" />
            </svg>
          </button>
        </div>
      </div>
    </main>
  );
}
