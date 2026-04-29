import { useRef, useEffect, useCallback, useState } from "react";
import { useAppStore } from "../../store";
import type { ChatMessage } from "../../types";
import {
  offloadManager,
  type MessageScope,
} from "../../messageOffload";

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

interface UseChatScrollOptions {
  allMessages: ChatMessage[];
  selectedChannel: number | null;
  selectedDmUser: number | null;
  currentScope: () => MessageScope | null;
}

export function useChatScroll({
  allMessages,
  selectedChannel,
  selectedDmUser,
  currentScope,
}: UseChatScrollOptions) {
  /** The scroll container (<div.messages>). */
  const messagesContainerRef = useRef<HTMLDivElement>(null);

  /** Bottom sentinel: always the last element inside the messages wrapper. */
  const bottomSentinelRef = useRef<HTMLDivElement>(null);

  /** Inner wrapper that grows with content. */
  const messagesInnerRef = useRef<HTMLDivElement>(null);

  /**
   * "Stick to bottom" flag.  When true, every content-height change
   * triggers an instant scroll to the bottom.
   */
  const stickToBottomRef = useRef(true);

  /**
   * Timestamp of the last programmatic scrollTo.  Scroll events within
   * 150 ms are not allowed to clear stickToBottomRef.
   */
  const lastProgrammaticScrollRef = useRef(0);

  /** Number of new (unread) messages received while scrolled up. */
  const [newMsgCount, setNewMsgCount] = useState(0);

  /**
   * The index in allMessages where a "new messages" divider should appear.
   * null = no divider.
   */
  const [lastReadIdx, setLastReadIdx] = useState<number | null>(null);

  /** Used to detect message count increases. */
  const prevMsgCountRef = useRef(0);

  /** Track the first message ID to detect history-prepend vs new-message-append. */
  const prevFirstMsgIdRef = useRef<string | null>(null);

  /** Set of message IDs currently being restored from offload storage. */
  const [restoringKeys, setRestoringKeys] = useState<Set<string>>(new Set());

  /**
   * Pending unread count captured when switching to a channel that had
   * unread messages.  Used to position the "new messages" divider on the
   * first message batch after the switch.
   */
  const pendingUnreadRef = useRef(0);

  /** Instant scroll-to-bottom, updating the programmatic-scroll timestamp. */
  const scrollToBottom = useCallback((el: HTMLElement) => {
    stickToBottomRef.current = true;
    lastProgrammaticScrollRef.current = Date.now();
    const sentinel = bottomSentinelRef.current;
    if (sentinel) {
      sentinel.scrollIntoView({ behavior: "instant", block: "end" });
    } else {
      el.scrollTo({ top: el.scrollHeight, behavior: "instant" });
    }
  }, []);

  // Track scroll position and detect user scroll-away gestures.
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
        stickToBottomRef.current = false;
      }
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

    if (delta <= 0) return;

    // Detect if older messages were prepended.
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

    const isInitialBatch = prevFirstId === null;
    const el = messagesContainerRef.current;

    // On the first batch after a channel switch, place the "new messages"
    // divider if there were pending unreads.
    if (isInitialBatch && pendingUnreadRef.current > 0 && count > pendingUnreadRef.current) {
      const pending = pendingUnreadRef.current;
      const dividerIdx = count - pending;
      pendingUnreadRef.current = 0;
      setLastReadIdx(dividerIdx);
      setNewMsgCount(pending);

      stickToBottomRef.current = false;
      requestAnimationFrame(() => {
        if (!el) return;
        const dividerEl = el.querySelector('[aria-label="New messages"]');
        if (dividerEl) {
          dividerEl.scrollIntoView({ behavior: "instant", block: "center" });
        } else {
          scrollToBottom(el);
        }
      });
      return;
    }
    pendingUnreadRef.current = 0;

    let atBottom: boolean;
    if (isInitialBatch) {
      atBottom = stickToBottomRef.current;
    } else {
      atBottom = el ? isWithinHalfViewport(el) : stickToBottomRef.current;
    }

    if (atBottom) {
      stickToBottomRef.current = true;
      requestAnimationFrame(() => {
        if (el) scrollToBottom(el);
      });
    } else {
      stickToBottomRef.current = false;
      setLastReadIdx((prev) => prev ?? count - delta);
      setNewMsgCount((prev) => prev + delta);
    }
  }, [allMessages, scrollToBottom]);

  // Re-pin after images / media load.
  useEffect(() => {
    const outer = messagesContainerRef.current;
    const inner = messagesInnerRef.current;
    if (!outer || !inner) return;

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

    const resizeObs = new ResizeObserver(repin);
    resizeObs.observe(inner);

    const trackedImages = new WeakSet<HTMLImageElement>();
    const trackedVideos = new WeakSet<HTMLVideoElement>();

    const trackImages = () => {
      for (const img of inner.querySelectorAll<HTMLImageElement>("img")) {
        if (trackedImages.has(img)) continue;
        trackedImages.add(img);
        if (!img.complete) {
          img.addEventListener("load", repin, { once: true });
        }
      }
      for (const vid of inner.querySelectorAll<HTMLVideoElement>("video")) {
        if (trackedVideos.has(vid)) continue;
        trackedVideos.add(vid);
        vid.addEventListener("loadedmetadata", repin, { once: true });
      }
    };

    trackImages();

    const mutObs = new MutationObserver(() => {
      trackImages();
      repin();
    });
    mutObs.observe(inner, { childList: true, subtree: true });

    return () => {
      resizeObs.disconnect();
      mutObs.disconnect();
    };
  }, []);

  // On channel / DM switch, reset scroll state.
  // Capture the pending unread count so the initial message batch can
  // place the "new messages" divider at the correct position.
  useEffect(() => {
    const { unreadCounts, dmUnreadCounts } = useAppStore.getState();
    if (selectedChannel !== null) {
      pendingUnreadRef.current = unreadCounts[selectedChannel] ?? 0;
    } else if (selectedDmUser !== null) {
      pendingUnreadRef.current = dmUnreadCounts[selectedDmUser] ?? 0;
    } else {
      pendingUnreadRef.current = 0;
    }

    setNewMsgCount(0);
    setLastReadIdx(null);
    // Reset to zero/null so the next message load is detected as an
    // initial batch (prevFirstId === null).
    prevMsgCountRef.current = 0;
    prevFirstMsgIdRef.current = null;
    stickToBottomRef.current = pendingUnreadRef.current === 0;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedChannel, selectedDmUser]);

  // Offload IntersectionObserver.
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
        rootMargin: "800px 0px 800px 0px",
      },
    );

    const observeAll = () => {
      for (const el of inner.querySelectorAll<HTMLElement>("[data-msg-id]")) {
        observer.observe(el);
      }
    };
    observeAll();

    const mutObs = new MutationObserver(observeAll);
    mutObs.observe(inner, { childList: true, subtree: true });

    return () => {
      observer.disconnect();
      mutObs.disconnect();
    };
  }, [selectedChannel, selectedDmUser]);

  /** Jump-to-bottom handler used by the "new messages" pill. */
  const handleScrollToBottom = useCallback(() => {
    const el = messagesContainerRef.current;
    if (el) scrollToBottom(el);
    setNewMsgCount(0);
    setLastReadIdx(null);
  }, [scrollToBottom]);

  return {
    messagesContainerRef,
    bottomSentinelRef,
    messagesInnerRef,
    newMsgCount,
    lastReadIdx,
    restoringKeys,
    handleScrollToBottom,
  };
}
