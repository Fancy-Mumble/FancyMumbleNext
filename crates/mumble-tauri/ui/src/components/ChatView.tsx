import React, { useState, useRef, useEffect, useCallback, useMemo, useReducer, type ClipboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../store";
import type { TimeFormat } from "../types";
import { getPreferences } from "../preferencesStorage";
import ChatHeader from "./ChatHeader";
import MessageItem from "./MessageItem";
import ChatComposer from "./ChatComposer";
import { usePersistentChat } from "./PersistentChatOverlays";
import type { PollPayload, PollVotePayload } from "./PollCreator";
import { registerVote, registerLocalVote, getPoll } from "./PollCard";
import { mediaKind, fileToDataUrl, fitImage, fitVideo, mediaToHtml } from "../utils/media";
import { textureToDataUrl } from "../profileFormat";
import { markdownToHtml } from "./MarkdownInput";
import {
  isHeavyContent,
  offloadManager,
  type MessageScope,
} from "../messageOffload";
import styles from "./ChatView.module.css";

// --- Scroll helpers ----------------------------------------------

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

interface ChatViewProps {
  readonly onChannelInfoToggle?: () => void;
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

export default function ChatView({ onChannelInfoToggle }: ChatViewProps) {
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

  // DM state
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);
  const dmMessages = useAppStore((s) => s.dmMessages);
  const sendDm = useAppStore((s) => s.sendDm);

  // Group chat state
  const selectedGroup = useAppStore((s) => s.selectedGroup);
  const groupMessages = useAppStore((s) => s.groupMessages);
  const sendGroupMessage = useAppStore((s) => s.sendGroupMessage);
  const groupChats = useAppStore((s) => s.groupChats);

  const isDmMode = selectedDmUser !== null;
  const isGroupMode = selectedGroup !== null;
  const dmPartner = isDmMode ? users.find((u) => u.session === selectedDmUser) : undefined;
  const activeGroup = isGroupMode ? groupChats.find((g) => g.id === selectedGroup) : undefined;

  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [, forceRender] = useReducer((c: number) => c + 1, 0);

  // Time display preferences (loaded once from persistent storage).
  const [timeFormat, setTimeFormat] = useState<TimeFormat>("auto");
  const [convertToLocalTime, setConvertToLocalTime] = useState(true);
  // System clock format resolved from OS (not from WebView Intl, which ignores
  // the Windows Region setting and always uses the language-tag default).
  const [systemUses24h, setSystemUses24h] = useState<boolean | undefined>(undefined);

  useEffect(() => {
    getPreferences().then((prefs) => {
      setTimeFormat(prefs.timeFormat);
      setConvertToLocalTime(prefs.convertToLocalTime);
    });
    invoke<"12h" | "24h" | null>("get_system_clock_format")
      .then((fmt) => {
        // null means non-Windows: leave systemUses24h as undefined so the
        // Intl probe in formatTimestamp is used instead.
        if (fmt !== null) setSystemUses24h(fmt === "24h");
      })
      .catch(() => { /* leave undefined - fall back to Intl */ });
  }, []);

  // --- Content offloading ------------------------------------------

  /** Set of message IDs currently being restored from offload storage. */
  const [restoringKeys, setRestoringKeys] = useState<Set<string>>(new Set());

  /** Build the `MessageScope` for the current chat mode. */
  const currentScope = useCallback((): MessageScope | null => {
    if (isGroupMode && selectedGroup) return { scope: "group", scopeId: selectedGroup };
    if (isDmMode && selectedDmUser !== null) return { scope: "dm", scopeId: String(selectedDmUser) };
    if (selectedChannel !== null) return { scope: "channel", scopeId: String(selectedChannel) };
    return null;
  }, [isGroupMode, selectedGroup, isDmMode, selectedDmUser, selectedChannel]);

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

  /** Track the first message ID to detect history-prepend vs new-message-append. */
  const prevFirstMsgIdRef = useRef<string | null>(null);

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

  /** Map session -> avatar data-URL for message avatars (cached). */
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

  // --- Smart scroll behaviour --------------------------------------
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
    const prevFirstId = prevFirstMsgIdRef.current;
    const curFirstId = count > 0 ? (allMessages[0].message_id ?? null) : null;

    prevMsgCountRef.current = count;
    prevFirstMsgIdRef.current = curFirstId;

    if (delta <= 0) return; // channel switch or deletion — no action

    // Detect if older messages were prepended (first message ID changed while
    // user was scrolled up).  In that case, preserve the scroll position so
    // the viewport doesn't jump.
    if (prevFirstId !== null && curFirstId !== prevFirstId) {
      const el = messagesContainerRef.current;
      if (el) {
        const prevScrollHeight = el.scrollHeight;
        requestAnimationFrame(() => {
          el.scrollTop += el.scrollHeight - prevScrollHeight;
        });
      }
      return;
    }

    // On the initial message batch (container was empty, prevFirstId null)
    // trust stickToBottomRef — the viewport check would fail because the
    // DOM was just populated and scrollTop is still 0.
    const isInitialBatch = prevFirstId === null;
    const el = messagesContainerRef.current;
    const atBottom = isInitialBatch
      ? stickToBottomRef.current
      : el ? isWithinHalfViewport(el) : stickToBottomRef.current;

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

  // --- Re-pin after images / media load -------------------------
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

  // On channel / DM switch, reset scroll state and jump to bottom instantly.
  useEffect(() => {
    setNewMsgCount(0);
    setLastReadIdx(null);
    prevMsgCountRef.current = allMessages.length;
    prevFirstMsgIdRef.current = allMessages.length > 0 ? (allMessages[0].message_id ?? null) : null;
    stickToBottomRef.current = true;
    requestAnimationFrame(() => {
      const el = messagesContainerRef.current;
      if (el) scrollToBottom(el);
    });
    // Only run when the selected channel, DM user, or group changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedChannel, selectedDmUser, selectedGroup]);

  // --- Offload IntersectionObserver ---------------------------------
  //
  // Watches message elements for visibility changes.  Heavy messages
  // that scroll out of view are offloaded (encrypted -> temp file) after
  // a delay; offloaded messages approaching the viewport are restored.

  const scopeRef = useRef(currentScope);
  scopeRef.current = currentScope;

  useEffect(() => {
    const inner = messagesInnerRef.current;
    const container = messagesContainerRef.current;
    if (!inner || !container) return;

    const refreshForScope = (scope: MessageScope) => {
      const state = useAppStore.getState();
      if (scope.scope === "channel") {
        state.refreshMessages(Number(scope.scopeId));
      } else if (scope.scope === "dm") {
        state.refreshDmMessages(Number(scope.scopeId));
      } else if (scope.scope === "group") {
        state.refreshGroupMessages(scope.scopeId);
      }
    };

    const handleRestored = (scope: MessageScope, restoredIds: string[]) => {
      setRestoringKeys((prev) => {
        const next = new Set(prev);
        for (const id of restoredIds) next.delete(id);
        return next;
      });
      if (restoredIds.length > 0) refreshForScope(scope);
    };

    const observer = new IntersectionObserver(
      (entries) => {
        const scope = scopeRef.current();
        if (!scope) return;

        // Collect all offloaded messages that just entered the viewport
        // so we can restore them in a single batch IPC call.
        const toRestore: string[] = [];

        for (const entry of entries) {
          const el = entry.target as HTMLElement;
          const msgId = el.dataset.msgId;
          if (!msgId) continue;

          if (entry.isIntersecting) {
            offloadManager.cancelOffload(msgId);

            if (offloadManager.isOffloaded(msgId)) {
              toRestore.push(msgId);
            }
          } else if (el.dataset.msgHeavy !== undefined) {
            offloadManager.scheduleOffload(msgId, scope, () => {
              refreshForScope(scope);
            });
          }
        }

        if (toRestore.length > 0) {
          setRestoringKeys((prev) => {
            const next = new Set(prev);
            for (const id of toRestore) next.add(id);
            return next;
          });

          offloadManager.restoreMany(toRestore, scope).then((results) => {
            handleRestored(scope, Object.keys(results));
          });
        }
      },
      {
        root: container,
        // Load content 800px before it enters the viewport to avoid
        // visible skeleton flicker; offload 200px after it leaves.
        rootMargin: "800px 0px 800px 0px",
      },
    );

    // Observe all message elements with a data-msg-id attribute.
    const observeAll = () => {
      for (const el of inner.querySelectorAll<HTMLElement>("[data-msg-id]")) {
        observer.observe(el);
      }
    };
    observeAll();

    // Re-observe when new message elements are added.
    const mutObs = new MutationObserver(observeAll);
    mutObs.observe(inner, { childList: true, subtree: true });

    return () => {
      observer.disconnect();
      mutObs.disconnect();
    };
  }, [selectedChannel, selectedDmUser, selectedGroup]);

  /** Jump-to-bottom handler used by the "new messages" pill. */
  const handleScrollToBottom = useCallback(() => {
    const el = messagesContainerRef.current;
    if (el) scrollToBottom(el);
    setNewMsgCount(0);
    setLastReadIdx(null);
  }, [scrollToBottom]);

  const handleSend = async () => {
    const text = draft.trim();
    if (!text) return;
    if (isGroupMode && selectedGroup !== null) {
      setDraft("");
      const html = markdownToHtml(text);
      await sendGroupMessage(selectedGroup, html);
    } else if (isDmMode && selectedDmUser !== null) {
      setDraft("");
      const html = markdownToHtml(text);
      await sendDm(selectedDmUser, html);
    } else if (selectedChannel !== null) {
      setDraft("");
      const html = markdownToHtml(text);
      await sendMessage(selectedChannel, html);
    }
  };

  /** Encode a File and send it as a media message. */
  const sendMediaFile = useCallback(
    async (file: File) => {
      if (!isGroupMode && !isDmMode && selectedChannel === null) return;

      const kind = mediaKind(file.type);
      if (!kind) {
        alert("Unsupported file type. Please select an image, GIF, or video.");
        return;
      }

      // 0 means "no special image limit" -> fall back to message_length.
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
        if (isGroupMode && selectedGroup !== null) {
          await sendGroupMessage(selectedGroup, html);
        } else if (isDmMode && selectedDmUser !== null) {
          await sendDm(selectedDmUser, html);
        } else if (selectedChannel !== null) {
          await sendMessage(selectedChannel, html);
        }
      } catch (err) {
        console.error("media send error:", err);
        alert(String(err));
      } finally {
        setSending(false);
      }
    },
    [isGroupMode, selectedGroup, sendGroupMessage, isDmMode, selectedDmUser, selectedChannel, serverConfig, sendMessage, sendDm],
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

  // --- GIF picker handler -----------------------------------------

  const handleGifSelect = useCallback(
    async (url: string, alt: string) => {
      const html = `<img src="${url}" alt="${alt}" />`;
      if (isGroupMode && selectedGroup !== null) {
        await sendGroupMessage(selectedGroup, html);
      } else if (isDmMode && selectedDmUser !== null) {
        await sendDm(selectedDmUser, html);
      } else if (selectedChannel !== null) {
        await sendMessage(selectedChannel, html);
      }
    },
    [isGroupMode, selectedGroup, sendGroupMessage, isDmMode, selectedDmUser, selectedChannel, sendMessage, sendDm],
  );

  // --- Poll handlers ---------------------------------------------

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

  // --- End poll handlers -------------------------------------------

  // Compute header values before any early returns (hooks can't be conditional).
  const [headerName, headerMemberCount] = computeHeader(
    isGroupMode, activeGroup, isDmMode, dmPartner, channel, memberCount,
  );
  const showJoinButton = !isDmMode && !isGroupMode && !isInChannel;

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
    <main className={styles.main}>
      <ChatHeader
        channelName={headerName}
        memberCount={headerMemberCount}
        isInChannel={isDmMode || isGroupMode || isInChannel}
        isDm={isDmMode}
        isGroup={isGroupMode}
        onJoin={showJoinButton ? () => joinChannel(selectedChannel!) : undefined}
        onChannelInfoToggle={onChannelInfoToggle}
        keyTrustLevel={persistent.trustLevel}
        onVerifyClick={persistent.onVerifyClick}
      />

      {/* Messages */}
      <div ref={messagesContainerRef} className={styles.messages}>
        <div ref={messagesInnerRef} className={styles.messagesInner}>
          {/* Persistent chat banners */}
          {persistent.banner}

          {/* Disputed key banner */}
          {persistent.disputeBanner}

          {allMessages.length === 0 ? (
            <div className={styles.empty}>
              <div className={styles.emptyIcon}>👋</div>
              <p>No messages yet. Say hello!</p>
            </div>
          ) : (
            allMessages.map((msg, i) => (
              <React.Fragment key={msg.message_id ?? `${msg.channel_id}-${msg.sender_session ?? "s"}-${msg.body.slice(0, 32)}-${i}`}>
                {/* Last-read divider */}
                {lastReadIdx !== null && i === lastReadIdx && (
                  <div className={styles.unreadDivider} aria-label="New messages">
                    <span className={styles.unreadDividerLabel}>New messages</span>
                  </div>
                )}
                <div
                  data-msg-id={msg.message_id ?? undefined}
                  data-msg-heavy={msg.message_id && isHeavyContent(msg.body) ? "" : undefined}
                >
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
                    timeFormat={timeFormat}
                    convertToLocalTime={convertToLocalTime}
                    systemUses24h={systemUses24h}
                    isRestoring={msg.message_id ? restoringKeys.has(msg.message_id) : false}
                  />
                </div>
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

      {/* Persistent chat dialogs (key verification, custodian prompt) */}
      {persistent.dialogs}
    </main>
  );
}
