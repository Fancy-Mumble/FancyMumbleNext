import { useState, useCallback, type ClipboardEvent } from "react";
import { useAppStore } from "../../store";
import type { ChatMessage } from "../../types";
import { markdownToHtml } from "./MarkdownInput";
import { mediaKind, fileToDataUrl, fitImage, fitVideo, mediaToHtml } from "../../utils/media";

interface UseChatSendOptions {
  pendingQuotes: ChatMessage[];
  clearQuotes: () => void;
  draft: string;
  clearDraft: () => void;
}

export function useChatSend({ pendingQuotes, clearQuotes, draft, clearDraft }: UseChatSendOptions) {
  const sendMessage = useAppStore((s) => s.sendMessage);
  const serverConfig = useAppStore((s) => s.serverConfig);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);
  const sendDm = useAppStore((s) => s.sendDm);
  const selectedGroup = useAppStore((s) => s.selectedGroup);
  const sendGroupMessage = useAppStore((s) => s.sendGroupMessage);

  const isDmMode = selectedDmUser !== null;
  const isGroupMode = selectedGroup !== null;

  const [sending, setSending] = useState(false);

  const handleSend = useCallback(async () => {
    const text = draft.trim();
    if (!text && pendingQuotes.length === 0) return;

    // Build quote markers and convert draft to HTML.
    const quoteMarkers = pendingQuotes
      .filter((q) => q.message_id)
      .map((q) => `<!-- FANCY_QUOTE:${q.message_id} -->`)
      .join("");
    const htmlBody = text ? markdownToHtml(text) : "";
    const html = quoteMarkers + htmlBody;
    if (!html) return;

    if (isGroupMode && selectedGroup !== null) {
      clearDraft();
      clearQuotes();
      await sendGroupMessage(selectedGroup, html);
    } else if (isDmMode && selectedDmUser !== null) {
      clearDraft();
      clearQuotes();
      await sendDm(selectedDmUser, html);
    } else if (selectedChannel !== null) {
      clearDraft();
      clearQuotes();
      await sendMessage(selectedChannel, html);
    }
  }, [draft, pendingQuotes, isGroupMode, selectedGroup, sendGroupMessage, isDmMode, selectedDmUser, sendDm, selectedChannel, sendMessage, clearDraft, clearQuotes]);

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

  return { sending, handleSend, sendMediaFile, handlePaste, handleGifSelect };
}
