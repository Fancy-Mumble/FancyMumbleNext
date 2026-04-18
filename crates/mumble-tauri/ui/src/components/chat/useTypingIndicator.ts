import { useRef, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../../store";
import { getPreferences } from "../../preferencesStorage";

/** Debounce interval between typing indicator messages (ms). */
const TYPING_DEBOUNCE_MS = 3000;

/**
 * Hook that sends typing indicators to the server when the user types.
 *
 * Call `notifyTyping()` on every input change. The hook debounces so at
 * most one message is sent per `TYPING_DEBOUNCE_MS` interval.
 *
 * Respects the `disableTypingIndicators` user preference.
 */
export function useTypingIndicator() {
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const status = useAppStore((s) => s.status);
  const lastSentRef = useRef(0);
  const channelRef = useRef(selectedChannel);
  const disabledRef = useRef(false);

  useEffect(() => {
    channelRef.current = selectedChannel;
  }, [selectedChannel]);

  useEffect(() => {
    getPreferences().then((prefs) => {
      disabledRef.current = prefs.disableTypingIndicators ?? false;
    });
  }, []);

  const notifyTyping = useCallback(() => {
    if (disabledRef.current) return;
    const channelId = channelRef.current;
    if (channelId == null || status !== "connected") return;

    const now = Date.now();
    if (now - lastSentRef.current < TYPING_DEBOUNCE_MS) return;
    lastSentRef.current = now;

    invoke("send_typing_indicator", { channelId })
      .then(() => console.debug("[typing] sent for channel", channelId))
      .catch((err) => console.error("[typing] invoke failed:", err));
  }, [status]);

  return notifyTyping;
}
