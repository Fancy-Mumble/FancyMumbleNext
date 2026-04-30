import React, { useState, useRef, useEffect, useCallback } from "react";
import { useAppStore } from "../../store";
import type { ChatMessage, ChannelEntry } from "../../types";
import { canDeleteMessages } from "../sidebar/ChannelEditorDialog";
import type { MessageContextMenuState } from "./MessageContextMenu";
import type { ToastData } from "../elements/Toast";
import styles from "./ChatView.module.css";

interface UseMessageSelectionOptions {
  selectedChannel: number | null;
  selectedDmUser: number | null;
  channel: ChannelEntry | undefined;
  messagesContainerRef: React.RefObject<HTMLDivElement | null>;
  setPendingQuotes: React.Dispatch<React.SetStateAction<ChatMessage[]>>;
}

export function useMessageSelection({
  selectedChannel,
  selectedDmUser,
  channel,
  messagesContainerRef,
  setPendingQuotes,
}: UseMessageSelectionOptions) {
  const deletePchatMessages = useAppStore((s) => s.deletePchatMessages);

  const canDelete = canDeleteMessages(channel);

  /** Whether bulk-selection mode is active. */
  const [selectionMode, setSelectionMode] = useState(false);
  /** Set of selected message IDs. */
  const [selectedMsgIds, setSelectedMsgIds] = useState<Set<string>>(new Set());
  /** Context menu state for right-clicking a message. */
  const [msgContextMenu, setMsgContextMenu] = useState<MessageContextMenuState | null>(null);
  /** Pending delete confirmation (single or bulk). */
  const [deleteConfirm, setDeleteConfirm] = useState<{ ids: string[] } | null>(null);
  const [toast, setToast] = useState<ToastData | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);
  const isDeletingRef = useRef(false);

  /** Toggle selection of a single message. */
  const toggleMsgSelection = useCallback((msgId: string) => {
    setSelectedMsgIds((prev) => {
      const next = new Set(prev);
      if (next.has(msgId)) next.delete(msgId);
      else next.add(msgId);
      return next;
    });
  }, []);

  /** Enter selection mode starting with a specific message. */
  const enterSelectionMode = useCallback((msg: ChatMessage) => {
    if (!msg.message_id) return;
    setSelectionMode(true);
    setSelectedMsgIds(new Set([msg.message_id]));
  }, []);

  /** Exit selection mode and clear selected messages. */
  const exitSelectionMode = useCallback(() => {
    setSelectionMode(false);
    setSelectedMsgIds(new Set());
  }, []);

  /** Handle right-click on a message bubble. */
  const handleMessageContextMenu = useCallback(
    (e: React.MouseEvent, msg: ChatMessage) => {
      if (!msg.message_id) return;
      e.preventDefault();
      setMsgContextMenu({ x: e.clientX, y: e.clientY, message: msg });
    },
    [],
  );

  /** Handle single-message delete from context menu. */
  const handleSingleDelete = useCallback((msg: ChatMessage) => {
    if (!msg.message_id) return;
    setDeleteConfirm({ ids: [msg.message_id] });
  }, []);

  /** Handle bulk delete from selection bar. */
  const handleBulkDelete = useCallback(() => {
    const ids = [...selectedMsgIds];
    if (ids.length === 0) return;
    setDeleteConfirm({ ids });
  }, [selectedMsgIds]);

  /** Confirm and execute the pending deletion. */
  const confirmDelete = useCallback(async () => {
    if (!deleteConfirm || selectedChannel === null || isDeletingRef.current) return;
    isDeletingRef.current = true;
    setIsDeleting(true);
    const count = deleteConfirm.ids.length;
    try {
      await deletePchatMessages(selectedChannel, { messageIds: deleteConfirm.ids });
      setToast({
        message: count === 1 ? "Message deleted" : `${count} messages deleted`,
        variant: "success",
      });
    } catch (err) {
      console.error("delete messages error:", err);
      setToast({ message: "Failed to delete messages", variant: "error" });
    } finally {
      isDeletingRef.current = false;
      setIsDeleting(false);
      setDeleteConfirm(null);
      exitSelectionMode();
    }
  }, [deleteConfirm, selectedChannel, deletePchatMessages, exitSelectionMode]);

  /** Long-press timer ref for touch selection. */
  const longPressTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  /** Start long-press timer on touch. */
  const handleTouchStart = useCallback(
    (msg: ChatMessage) => {
      if (selectionMode || !msg.message_id) return;
      longPressTimerRef.current = setTimeout(() => {
        setMsgContextMenu({ x: window.innerWidth / 2, y: window.innerHeight / 2, message: msg });
        longPressTimerRef.current = null;
      }, 500);
    },
    [selectionMode],
  );

  /** Cancel long-press timer. */
  const cancelLongPress = useCallback(() => {
    if (longPressTimerRef.current !== null) {
      clearTimeout(longPressTimerRef.current);
      longPressTimerRef.current = null;
    }
  }, []);

  // --- Message action bar handlers ---------------------------------

  /** Called when the cite/quote button is clicked. */
  const handleCite = useCallback((msg: ChatMessage) => {
    if (!msg.message_id) return;
    setPendingQuotes((prev) => {
      if (prev.some((q) => q.message_id === msg.message_id)) return prev;
      return [...prev, msg];
    });
  }, [setPendingQuotes]);

  /** Copy message text to clipboard from kebab menu. */
  const handleCopyText = useCallback((msg: ChatMessage) => {
    const plain = msg.body
      .replaceAll(/<[^>]*>/g, "")
      .replaceAll("&lt;", "<")
      .replaceAll("&gt;", ">")
      .replaceAll("&amp;", "&")
      .trim();
    navigator.clipboard.writeText(plain).catch(() => {
      /* clipboard write may fail silently */
    });
  }, []);

  /** Scroll to a quoted message and flash-highlight it. */
  const handleScrollToMessage = useCallback((messageId: string) => {
    const container = messagesContainerRef.current;
    if (!container) return;
    const el = container.querySelector<HTMLElement>(
      `[data-msg-id="${CSS.escape(messageId)}"]`,
    );
    if (!el) return;
    el.scrollIntoView({ behavior: "smooth", block: "center" });
    el.classList.add(styles.quoteHighlight);
    setTimeout(() => el.classList.remove(styles.quoteHighlight), 1500);
  }, [messagesContainerRef]);

  /** Remove a pending quote by message ID. */
  const removePendingQuote = useCallback((msgId: string) => {
    setPendingQuotes((prev) => prev.filter((p) => p.message_id !== msgId));
  }, [setPendingQuotes]);

  /** Auto-exit selection mode when all messages are deselected. */
  useEffect(() => {
    if (selectionMode && selectedMsgIds.size === 0) {
      setSelectionMode(false);
    }
  }, [selectionMode, selectedMsgIds]);

  /** Clear selection when switching channels. */
  useEffect(() => {
    exitSelectionMode();
  }, [selectedChannel, selectedDmUser, exitSelectionMode]);

  /** Clear pending quotes when the active conversation changes. */
  useEffect(() => {
    setPendingQuotes([]);
  }, [selectedChannel, selectedDmUser, setPendingQuotes]);

  // --- Text-selection bulk trigger ---------------------------------
  useEffect(() => {
    const container = messagesContainerRef.current;
    if (!container || !canDelete) return;

    const findMsgId = (node: Node | null): string | null => {
      let el: HTMLElement | null = node instanceof HTMLElement ? node : node?.parentElement ?? null;
      while (el && el !== container) {
        if (el.dataset.msgId) return el.dataset.msgId;
        el = el.parentElement;
      }
      return null;
    };

    const handleSelectionChange = () => {
      if (selectionMode) return;
      const sel = globalThis.getSelection();
      if (!sel || sel.isCollapsed || sel.rangeCount === 0) return;

      const range = sel.getRangeAt(0);
      if (!container.contains(range.commonAncestorContainer)) return;

      const anchorId = findMsgId(sel.anchorNode);
      const focusId = findMsgId(sel.focusNode);
      if (!anchorId || !focusId || anchorId === focusId) return;

      const msgElements = Array.from(container.querySelectorAll<HTMLElement>("[data-msg-id]"));
      const anchorIdx = msgElements.findIndex((el) => el.dataset.msgId === anchorId);
      const focusIdx = msgElements.findIndex((el) => el.dataset.msgId === focusId);
      if (anchorIdx < 0 || focusIdx < 0) return;

      const lo = Math.min(anchorIdx, focusIdx);
      const hi = Math.max(anchorIdx, focusIdx);
      const ids = new Set<string>();
      for (let i = lo; i <= hi; i++) {
        const id = msgElements[i].dataset.msgId;
        if (id) ids.add(id);
      }

      if (ids.size >= 2) {
        sel.removeAllRanges();
        setSelectionMode(true);
        setSelectedMsgIds(ids);
      }
    };

    document.addEventListener("selectionchange", handleSelectionChange);
    return () => document.removeEventListener("selectionchange", handleSelectionChange);
  }, [selectionMode, canDelete, messagesContainerRef]);

  const closeContextMenu = useCallback(() => setMsgContextMenu(null), []);
  const clearDeleteConfirm = useCallback(() => setDeleteConfirm(null), []);
  const clearToast = useCallback(() => setToast(null), []);
  const showToast = useCallback((data: ToastData) => setToast(data), []);

  return {
    canDelete,
    selectionMode,
    selectedMsgIds,
    msgContextMenu,
    deleteConfirm,
    isDeleting,
    toast,
    toggleMsgSelection,
    enterSelectionMode,
    exitSelectionMode,
    handleMessageContextMenu,
    handleSingleDelete,
    handleBulkDelete,
    confirmDelete,
    handleTouchStart,
    cancelLongPress,
    handleCite,
    handleCopyText,
    handleScrollToMessage,
    removePendingQuote,
    closeContextMenu,
    clearDeleteConfirm,
    clearToast,
    showToast,
  };
}
