/**
 * Hook encapsulating all message-reaction logic:
 * sending/receiving reactions via native PchatReaction proto,
 * toggling emoji, and managing the emoji picker overlay.
 */

import type React from "react";
import { useState, useCallback, useRef } from "react";
import { useAppStore } from "../../store";
import type { ChatMessage } from "../../types";
import { isMobile } from "../../utils/platform";
import {
  hasReacted,
  getReactions,
  type ReactionSummary,
} from "./reactionStore";

interface EmojiPickerState {
  /** Message being reacted to. */
  message: ChatMessage;
  /** Anchor coordinates for positioning the picker. */
  x: number;
  y: number;
}

export function useReactions() {
  const users = useAppStore((s) => s.users);
  const ownSession = useAppStore((s) => s.ownSession);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const sendReaction = useAppStore((s) => s.sendReaction);
  const reactionVersion = useAppStore((s) => s.reactionVersion);

  /** Emoji picker state (null = closed). */
  const [emojiPicker, setEmojiPicker] = useState<EmojiPickerState | null>(null);

  const usersRef = useRef(users);
  usersRef.current = users;

  /**
   * Send a reaction (add or remove) via the native PchatReaction proto.
   * The server broadcasts PchatReactionDeliver to all channel members;
   * the local store updates when the event arrives.
   */
  const doSendReaction = useCallback(
    async (messageId: string, emoji: string, action: "add" | "remove", channelId: number) => {
      if (ownSession === null) return;
      await sendReaction(channelId, messageId, emoji, action);
    },
    [ownSession, sendReaction],
  );

  /**
   * Toggle a reaction on/off for the current user.
   * Used by both quick-reaction buttons and the reaction pill toggle.
   */
  const toggleReaction = useCallback(
    async (msg: ChatMessage, emoji: string) => {
      if (!msg.message_id) return;
      const channelId = msg.channel_id ?? selectedChannel ?? 0;
      const ownUser = ownSession !== null ? users.find((u) => u.session === ownSession) : undefined;
      const ownHash = ownUser?.hash ?? "";

      const alreadyReacted = !!ownHash && hasReacted(msg.message_id, emoji, ownHash);
      await doSendReaction(msg.message_id, emoji, alreadyReacted ? "remove" : "add", channelId);
    },
    [ownSession, users, selectedChannel, doSendReaction],
  );

  /** Handle a quick-reaction emoji click from the action bar. */
  const handleReaction = useCallback(
    (msg: ChatMessage, emoji: string) => {
      void toggleReaction(msg, emoji);
    },
    [toggleReaction],
  );

  /** Open the full emoji picker, anchored to the click position. */
  const handleMoreReactions = useCallback((msg: ChatMessage, e?: React.MouseEvent) => {
    if (isMobile || !e) {
      setEmojiPicker({ message: msg, x: window.innerWidth / 2, y: window.innerHeight / 2 });
      return;
    }
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    setEmojiPicker({ message: msg, x: rect.right, y: rect.bottom });
  }, []);

  /** Open emoji picker with specific anchor coordinates. */
  const openEmojiPickerAt = useCallback((msg: ChatMessage, x: number, y: number) => {
    setEmojiPicker({ message: msg, x, y });
  }, []);

  /** Close the emoji picker. */
  const closeEmojiPicker = useCallback(() => setEmojiPicker(null), []);

  /** Called when an emoji is selected from the full picker. */
  const handleEmojiSelect = useCallback(
    (emoji: string) => {
      if (!emojiPicker) return;
      void toggleReaction(emojiPicker.message, emoji);
      setEmojiPicker(null);
    },
    [emojiPicker, toggleReaction],
  );

  /** Get reaction summaries for a given message (convenience wrapper). */
  const getMessageReactions = useCallback(
    (messageId: string): ReactionSummary[] => getReactions(messageId),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reactionVersion triggers re-computation
    [reactionVersion],
  );

  return {
    emojiPicker,
    handleReaction,
    handleMoreReactions,
    openEmojiPickerAt,
    closeEmojiPicker,
    handleEmojiSelect,
    toggleReaction,
    getMessageReactions,
  };
}
