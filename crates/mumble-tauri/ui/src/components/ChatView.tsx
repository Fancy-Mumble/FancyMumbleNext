import React, { useState, useRef, useEffect, useCallback, useMemo, useReducer, type ClipboardEvent } from "react";
import { useAppStore } from "../store";
import ChatHeader from "./ChatHeader";
import MessageItem from "./MessageItem";
import ChatComposer from "./ChatComposer";
import type { PollPayload, PollVotePayload } from "./PollCreator";
import { registerVote, registerLocalVote, getPoll } from "./PollCard";
import { mediaKind, fileToDataUrl, fitImage, fitVideo, mediaToHtml } from "../utils/media";
import { textureToDataUrl } from "../profileFormat";
import { markdownToHtml } from "./MarkdownInput";
import styles from "./ChatView.module.css";

// ─── Scroll helpers ──────────────────────────────────────────────

/** Pixel threshold: user counts as "at the bottom" when within this. */
const NEAR_BOTTOM_PX = 120;

/** Returns true when the scrollable container is near the bottom. */
function isNearBottom(el: HTMLElement): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_BOTTOM_PX;
}

/**
 * Stricter check: the user must be within half the visible viewport of the
 * bottom.  Used by auto-scroll triggers to avoid pulling the user down when
 * they have deliberately scrolled up.
 */
function isWithinHalfViewport(el: HTMLElement): boolean {
  const threshold = Math.max(el.clientHeight / 2, NEAR_BOTTOM_PX);
  return el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
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
  const selectUser = useAppStore((s) => s.selectUser);

  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [, forceRender] = useReducer((c: number) => c + 1, 0);

  /** The scroll container (<div.messages>). */
  const messagesContainerRef = useRef<HTMLDivElement>(null);

  /** Bottom sentinel: always the last element inside the messages wrapper.
   *  Used as the scroll target so the browser resolves the final position
   *  from the actual DOM element, not a potentially stale scrollHeight. */
  const bottomSentinelRef = useRef<HTMLDivElement>(null);

  /**
   * "Stick to bottom" flag.  When true, every content-height change
   * (image decode, new message, etc.) triggers an instant scroll to the
   * bottom.  The flag is set to `false` ONLY by user-initiated scroll-up
   * gestures (wheel / touch) or scroll events clearly from manual
   * interaction (scrollbar drag).  This avoids the classic race condition
   * where a programmatic `scrollTo` + subsequent async image decode +
   * scroll event would corrupt a simple `isNearBottom` check.
   */
  const stickToBottomRef = useRef(true);

  /**
   * Timestamp of the last programmatic `scrollTo`.  Scroll events that
   * fire within the grace window (150 ms) after a programmatic scroll
   * are NOT allowed to clear `stickToBottomRef` -- they might be stale
   * due to an image decoding between the scrollTo and the event dispatch.
   */
  const lastProgrammaticScrollRef = useRef(0);

  /** Number of new (unread) messages received while the user was scrolled up. */
  const [newMsgCount, setNewMsgCount] = useState(0);

  /**
   * The index in `allMessages` where a "new messages" divider should appear.
   * null = no divider (user was at the bottom when all messages arrived).
   */
  const [lastReadIdx, setLastReadIdx] = useState<number | null>(null);

  /** Used to detect message count increases. */
  const prevMsgCountRef = useRef(0);

  /** Instant scroll-to-bottom, updating the programmatic-scroll timestamp. */
  const scrollToBottom = useCallback((el: HTMLElement) => {
    stickToBottomRef.current = true;
    lastProgrammaticScrollRef.current = Date.now();
    // Use the sentinel if available (more reliable than scrollHeight
    // when images are still decoding).  Falls back to scrollTo.
    const sentinel = bottomSentinelRef.current;
    if (sentinel) {
      sentinel.scrollIntoView({ behavior: "instant", block: "end" });
    } else {
      el.scrollTo({ top: el.scrollHeight, behavior: "instant" });
    }
  }, []);

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

  /** Merge real messages with local-only poll messages for rendering. */
  const allMessages = useMemo(() => {
    const channelPolls = pollMessages.filter(
      (m) => m.channel_id === selectedChannel,
    );
    return [...messages, ...channelPolls];
  }, [messages, pollMessages, selectedChannel]);

  // ─── Smart scroll behaviour ──────────────────────────────────────
  //
  // 1. When the user is at the bottom and new messages arrive, auto-scroll.
  // 2. When the user has scrolled up, do NOT auto-scroll.  Instead, show a
  //    "N new messages" pill and record a last-read divider index.
  // 3. When the user scrolls back to the bottom, dismiss the pill and clear
  //    the divider.
  // 4. A ResizeObserver re-pins the scroll after images/iframes load so
  //    the view stays pinned to the actual bottom.

  // Track scroll position and detect user scroll-away gestures.
  //
  // Three listeners cooperate:
  //   scroll  - re-enables stickToBottom when the user reaches the bottom;
  //             disables it when user is NOT near bottom AND no recent
  //             programmatic scroll (catches scrollbar drag / keyboard).
  //   wheel   - immediately disables stickToBottom on upward wheel
  //             (most common desktop scroll input).
  //   touch   - immediately disables stickToBottom on upward swipe
  //             (mobile / touchscreen).
  //
  // Programmatic scrollTo NEVER fires wheel/touch events, so those
  // handlers are immune to the image-decode race condition.  The scroll
  // handler uses a 150 ms grace window after the last programmatic
  // scroll to avoid false negatives from stale events.
  useEffect(() => {
    const el = messagesContainerRef.current;
    if (!el) return;

    const onScroll = () => {
      const atBottom = isNearBottom(el);
      if (atBottom) {
        stickToBottomRef.current = true;
        if (newMsgCount > 0) {
          setNewMsgCount(0);
          setLastReadIdx(null);
        }
      } else if (Date.now() - lastProgrammaticScrollRef.current > 150) {
        // Not near the bottom AND no recent programmatic scroll.
        // This is a genuine user scroll-away (scrollbar drag, keyboard,
        // or a wheel event we already handled).
        stickToBottomRef.current = false;
      }
      // If within the grace window we leave stickToBottom unchanged --
      // the scroll event is likely a stale artifact of a programmatic
      // scroll + concurrent image decode.
    };

    const onWheel = (e: WheelEvent) => {
      if (e.deltaY < 0) stickToBottomRef.current = false;
    };

    let lastTouchY = 0;
    const onTouchStart = (e: TouchEvent) => {
      lastTouchY = e.touches[0].clientY;
    };
    const onTouchMove = (e: TouchEvent) => {
      const currentY = e.touches[0].clientY;
      // Finger moving down = content scrolling up.
      if (currentY > lastTouchY + 5) stickToBottomRef.current = false;
      lastTouchY = currentY;
    };

    el.addEventListener("scroll", onScroll, { passive: true });
    el.addEventListener("wheel", onWheel, { passive: true });
    el.addEventListener("touchstart", onTouchStart, { passive: true });
    el.addEventListener("touchmove", onTouchMove, { passive: true });
    return () => {
      el.removeEventListener("scroll", onScroll);
      el.removeEventListener("wheel", onWheel);
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("touchmove", onTouchMove);
    };
  }, [newMsgCount]);

  // React to message-count changes.
  useEffect(() => {
    const count = allMessages.length;
    const delta = count - prevMsgCountRef.current;
    prevMsgCountRef.current = count;
    if (delta <= 0) return; // channel switch or first load - no action

    // Re-check the scroll position right now (not from the cached ref)
    // because the DOM may not have processed a scroll event yet.
    const el = messagesContainerRef.current;
    const atBottom = el ? isWithinHalfViewport(el) : stickToBottomRef.current;

    if (atBottom) {
      // User is at bottom - auto-scroll after the DOM updates.
      stickToBottomRef.current = true;
      requestAnimationFrame(() => {
        if (el) scrollToBottom(el);
      });
    } else {
      // User has scrolled up - record a divider and bump the pill counter.
      stickToBottomRef.current = false;
      setLastReadIdx((prev) => prev ?? count - delta);
      setNewMsgCount((prev) => prev + delta);
    }
  }, [allMessages]);

  /** Inner wrapper that grows with content. */
  const messagesInnerRef = useRef<HTMLDivElement>(null);

  // ─── Re-pin after images / media load ─────────────────────────
  //
  // When the user is pinned to the bottom (`stickToBottomRef`), every
  // content-height change must scroll down.  Three independent
  // mechanisms guarantee this:
  //
  //   1. ResizeObserver on the inner wrapper -- catches any height
  //      change (images decoding, embeds, font load, etc.).  Fires
  //      after layout, so scrollHeight is fresh.
  //
  //   2. Per-image `load` handlers -- attached after each message-list
  //      change via a MutationObserver.  Catches every individual
  //      <img>/<video> load.  Wrapped in rAF so scrollHeight reflects
  //      the newly loaded resource.
  //
  //   3. MutationObserver on the inner wrapper -- detects when React
  //      adds new DOM nodes (new messages) and immediately scans for
  //      unloaded <img> elements to attach `load` handlers to.
  //
  // Together these make the auto-scroll robust against every timing
  // variant: synchronous data-URL decodes, slow network images,
  // batched React commits, interleaved image decodes, etc.
  useEffect(() => {
    const outer = messagesContainerRef.current;
    const inner = messagesInnerRef.current;
    if (!outer || !inner) return;

    // Helper: scroll to bottom via the sentinel, with a rAF to
    // guarantee layout is settled.
    const repin = () => {
      if (!stickToBottomRef.current) return;
      requestAnimationFrame(() => {
        if (!stickToBottomRef.current) return;
        lastProgrammaticScrollRef.current = Date.now();
        const sentinel = bottomSentinelRef.current;
        if (sentinel) {
          sentinel.scrollIntoView({ behavior: "instant", block: "end" });
        } else {
          outer.scrollTo({ top: outer.scrollHeight, behavior: "instant" });
        }
      });
    };

    // (1) ResizeObserver
    const resizeObs = new ResizeObserver(repin);
    resizeObs.observe(inner);

    // (2) + (3) Scan for unloaded images and attach load handlers.
    //     Called on initial mount and whenever new nodes are added.
    const trackedImages = new WeakSet<HTMLImageElement>();
    const trackedVideos = new WeakSet<HTMLVideoElement>();

    const trackImages = () => {
      const imgs = inner.querySelectorAll<HTMLImageElement>("img");
      for (const img of imgs) {
        if (trackedImages.has(img)) continue;
        trackedImages.add(img);
        if (!img.complete) {
          img.addEventListener("load", repin, { once: true });
        }
      }
      // Also track <video> elements
      const vids = inner.querySelectorAll<HTMLVideoElement>("video");
      for (const vid of vids) {
        if (trackedVideos.has(vid)) continue;
        trackedVideos.add(vid);
        vid.addEventListener("loadedmetadata", repin, { once: true });
      }
    };

    // Initial scan
    trackImages();

    // (3) MutationObserver to scan new nodes as React adds them
    const mutObs = new MutationObserver(() => {
      trackImages();
      // Also repin because new DOM content may have changed height
      repin();
    });
    mutObs.observe(inner, { childList: true, subtree: true });

    return () => {
      resizeObs.disconnect();
      mutObs.disconnect();
    };
  }, []);

  // On channel switch, reset scroll state and jump to bottom instantly.
  useEffect(() => {
    setNewMsgCount(0);
    setLastReadIdx(null);
    prevMsgCountRef.current = allMessages.length;
    stickToBottomRef.current = true;
    requestAnimationFrame(() => {
      const el = messagesContainerRef.current;
      if (el) scrollToBottom(el);
    });
    // Only run when the selected channel changes, not allMessages.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedChannel]);

  /** Jump-to-bottom handler used by the "new messages" pill. */
  const handleScrollToBottom = useCallback(() => {
    const el = messagesContainerRef.current;
    if (el) scrollToBottom(el);
    setNewMsgCount(0);
    setLastReadIdx(null);
  }, [scrollToBottom]);

  const handleSend = async () => {
    const text = draft.trim();
    if (!text || selectedChannel === null) return;
    setDraft("");
    const html = markdownToHtml(text);
    await sendMessage(selectedChannel, html);
  };

  /** Encode a File and send it as a media message. */
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
      forceRender();

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
      <ChatHeader
        channelName={channel?.name ?? "Unknown"}
        memberCount={memberCount}
        isInChannel={isInChannel}
        onJoin={() => joinChannel(selectedChannel)}
      />

      {/* Messages */}
      <div ref={messagesContainerRef} className={styles.messages}>
        <div ref={messagesInnerRef} className={styles.messagesInner}>
          {allMessages.length === 0 ? (
            <div className={styles.empty}>
              <div className={styles.emptyIcon}>👋</div>
              <p>No messages yet. Say hello!</p>
            </div>
          ) : (
            allMessages.map((msg, i) => (
              <React.Fragment key={`${msg.channel_id}-${msg.sender_session ?? "s"}-${msg.body.slice(0, 32)}-${i}`}>
                {/* Last-read divider */}
                {lastReadIdx !== null && i === lastReadIdx && (
                  <div className={styles.unreadDivider} aria-label="New messages">
                    <span className={styles.unreadDividerLabel}>New messages</span>
                  </div>
                )}
                <MessageItem
                  msg={msg}
                  index={i}
                  avatarUrl={
                    msg.sender_session === null
                      ? undefined
                      : avatarBySession.get(msg.sender_session)
                  }
                  user={
                    msg.sender_session === null
                      ? undefined
                      : userBySession.get(msg.sender_session)
                  }
                  polls={polls}
                  ownSession={ownSession}
                  onVote={handlePollVote}
                  onAvatarClick={selectUser}
                />
              </React.Fragment>
            ))
          )}
          {/* Bottom sentinel - scroll target for auto-scroll */}
          <div ref={bottomSentinelRef} aria-hidden="true" style={{ height: 0, overflow: "hidden" }} />
        </div>
      </div>

      {/* "New messages" pill - shown when user scrolled up and messages arrive */}
      {newMsgCount > 0 && (
        <button
          className={styles.newMessagesPill}
          onClick={handleScrollToBottom}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor"
            strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <polyline points="6 9 12 15 18 9" />
          </svg>
          {newMsgCount} new {newMsgCount === 1 ? "message" : "messages"}
        </button>
      )}

      <ChatComposer
        draft={draft}
        onChange={setDraft}
        onSend={handleSend}
        onPaste={handlePaste}
        onFileSelected={sendMediaFile}
        onGifSelect={handleGifSelect}
        onPollCreate={handlePollCreate}
        disabled={sending}
      />
    </main>
  );
}
