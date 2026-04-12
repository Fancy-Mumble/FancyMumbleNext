import { useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getPreferences } from "../../preferencesStorage";

/**
 * Hook that automatically sends read receipts and queries existing
 * read states when the user views a channel.
 *
 * - On channel switch: queries the server for existing read states.
 * - When new messages arrive (or on first view): sends a read receipt
 *   with the latest message ID as the watermark.
 * - When the browser tab / app becomes visible again, re-sends the
 *   current watermark so the sender sees the checkmark.
 * - Respects the `disableReadReceipts` user preference.
 */
export function useReadReceipts(
  channelId: number | null,
  lastMessageId: string | undefined,
) {
  const channelRef = useRef<number | null>(null);
  const lastSentRef = useRef<string | undefined>(undefined);
  const disabledRef = useRef(false);

  // Load preference once, then cache it.
  useEffect(() => {
    getPreferences().then((prefs) => {
      disabledRef.current = prefs.disableReadReceipts ?? false;
    });
  }, []);

  const sendReceipt = useCallback((chId: number, msgId: string) => {
    if (disabledRef.current) return;
    invoke("send_read_receipt", {
      channelId: chId,
      lastReadMessageId: msgId,
    }).catch((e) => console.error("send_read_receipt error:", e));
  }, []);

  // Query read receipts when switching channels.
  useEffect(() => {
    if (channelId == null) return;
    if (channelId === channelRef.current) return;
    channelRef.current = channelId;
    lastSentRef.current = undefined;

    invoke("query_read_receipts", { channelId }).catch((e) =>
      console.error("query_read_receipts error:", e),
    );
  }, [channelId]);

  // Send read receipt when the latest message changes or when
  // the user first views a channel with existing messages.
  // Only send when the page is actually visible to the user.
  useEffect(() => {
    if (channelId == null || !lastMessageId) return;
    if (document.visibilityState !== "visible") return;
    if (lastMessageId === lastSentRef.current) return;
    lastSentRef.current = lastMessageId;
    sendReceipt(channelId, lastMessageId);
  }, [channelId, lastMessageId, sendReceipt]);

  // Re-send the watermark when the page becomes visible again
  // (user alt-tabbed back, switched browser tab, etc.).
  useEffect(() => {
    const handleVisibility = () => {
      if (document.visibilityState !== "visible") return;
      const chId = channelRef.current;
      const msgId = lastSentRef.current;
      if (chId != null && msgId) {
        sendReceipt(chId, msgId);
      }
    };
    document.addEventListener("visibilitychange", handleVisibility);
    return () => document.removeEventListener("visibilitychange", handleVisibility);
  }, [sendReceipt]);
}
