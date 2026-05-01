import { useState, useCallback, useEffect, useRef, type ClipboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "../../store";
import type { AclData, AclGroup, ChatMessage } from "../../types";
import type { ToastData } from "../elements/Toast";
import { markdownToHtml } from "./MarkdownInput";
import { mediaKind, fileToDataUrl, fitImage, fitVideo, mediaToHtml } from "../../utils/media";
import { applyMentionsToHtml, type MentionResolver } from "../../utils/mentions";
import { rootChannelId } from "../../pages/admin/rootChannel";

interface UseChatSendOptions {
  pendingQuotes: ChatMessage[];
  clearQuotes: () => void;
  draft: string;
  clearDraft: () => void;
  editingMessage?: ChatMessage | null;
  onEditComplete?: () => void;
  showToast?: (data: ToastData) => void;
}

export function useChatSend({ pendingQuotes, clearQuotes, draft, clearDraft, editingMessage, onEditComplete, showToast }: UseChatSendOptions) {
  const sendMessage = useAppStore((s) => s.sendMessage);
  const editMessage = useAppStore((s) => s.editMessage);
  const serverConfig = useAppStore((s) => s.serverConfig);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);
  const sendDm = useAppStore((s) => s.sendDm);
  const users = useAppStore((s) => s.users);
  const channels = useAppStore((s) => s.channels);
  const addPendingPlaceholder = useAppStore((s) => s.addPendingPlaceholder);
  const markPendingFailed = useAppStore((s) => s.markPendingFailed);
  const dismissPendingMessage = useAppStore((s) => s.dismissPendingMessage);
  const rootId = (() => rootChannelId(channels))();

  // Subscribe to root-channel ACL so the resolver can attach role colors.
  const [roleGroups, setRoleGroups] = useState<readonly AclGroup[]>([]);
  useEffect(() => {
    let cancelled = false;
    const unlisten = listen<AclData>("acl", (event) => {
      if (!cancelled && event.payload.channel_id === rootId) {
        setRoleGroups(event.payload.groups);
      }
    });
    invoke("request_acl", { channelId: rootId }).catch(() => {});
    return () => {
      cancelled = true;
      unlisten.then((f) => f());
    };
  }, [rootId]);

  // Resolver used to convert <@SESSION> markers into named chips on send.
  const mentionResolver = useRef<MentionResolver>({
    resolveSession: () => null,
  });
  mentionResolver.current = {
    resolveSession: (session) => {
      const u = users.find((x) => x.session === session);
      return u ? { name: u.name } : null;
    },
    resolveRole: (name) => {
      const g = roleGroups.find((x) => x.name === name);
      return g ? { color: g.color ?? null } : null;
    },
  };

  const renderBody = useCallback(
    (text: string) => applyMentionsToHtml(markdownToHtml(text), mentionResolver.current),
    [],
  );

  const isDmMode = selectedDmUser !== null;

  const [sending, setSending] = useState(false);

  const handleSend = useCallback(async () => {
    const text = draft.trim();
    if (!text && pendingQuotes.length === 0) return;

    // Edit mode: update the existing message instead of sending a new one.
    if (editingMessage?.message_id && text) {
      const htmlBody = renderBody(text);
      const channelId = editingMessage.channel_id;
      if (channelId != null) {
        clearDraft();
        onEditComplete?.();
        await editMessage(channelId, editingMessage.message_id, htmlBody);
      }
      return;
    }

    // Build quote markers and convert draft to HTML.
    const quoteMarkers = pendingQuotes
      .filter((q) => q.message_id)
      .map((q) => `<!-- FANCY_QUOTE:${q.message_id} -->`)
      .join("");
    const htmlBody = text ? renderBody(text) : "";
    const html = quoteMarkers + htmlBody;
    if (!html) return;

    if (isDmMode && selectedDmUser !== null) {
      clearDraft();
      clearQuotes();
      await sendDm(selectedDmUser, html);
    } else if (selectedChannel !== null) {
      clearDraft();
      clearQuotes();
      await sendMessage(selectedChannel, html);
    }
  }, [draft, pendingQuotes, editingMessage, editMessage, onEditComplete, isDmMode, selectedDmUser, sendDm, selectedChannel, sendMessage, clearDraft, clearQuotes, renderBody]);

  const sendMediaFile = useCallback(
    async (file: File) => {
      if (!isDmMode && selectedChannel === null) return;

      const kind = mediaKind(file.type);
      if (!kind) {
        const msg = "Unsupported file type. Pick an image, GIF, or video.";
        if (showToast) showToast({ message: msg, variant: "error" });
        else console.error(msg);
        return;
      }

      // 0 means "no special image limit" -> fall back to message_length.
      const maxBytes =
        serverConfig.max_image_message_length > 0
          ? serverConfig.max_image_message_length
          : serverConfig.max_message_length;

      // Render an immediate "preparing" placeholder so the user sees
      // feedback BEFORE the (possibly multi-second) JPEG re-encoding
      // starts.  Without this, mobile users would tap "send" and see
      // nothing happen for several seconds while the JS thread is
      // busy compressing a large camera photo.
      const preparingLabel =
        kind === "video" ? "Preparing video\u2026" : "Preparing image\u2026";
      const placeholderBody = `<em>${preparingLabel}</em>`;
      const channelTarget = isDmMode ? null : selectedChannel;
      const dmTarget = isDmMode ? selectedDmUser : null;
      const placeholderId = addPendingPlaceholder(channelTarget, dmTarget, placeholderBody);

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
        // Hand off to the real send path; it will create its own pending
        // placeholder for the network phase.
        dismissPendingMessage(placeholderId);
        if (isDmMode && selectedDmUser !== null) {
          await sendDm(selectedDmUser, html);
        } else if (selectedChannel !== null) {
          await sendMessage(selectedChannel, html);
        }
      } catch (err) {
        console.error("media send error:", err);
        const detail = err instanceof Error ? err.message : String(err);
        markPendingFailed(placeholderId, `Couldn't prepare media: ${detail}`);
        if (showToast) {
          showToast({
            message: `Failed to send media: ${detail}`,
            variant: "error",
          });
        }
      } finally {
        setSending(false);
      }
    },
    [
      isDmMode,
      selectedDmUser,
      selectedChannel,
      serverConfig,
      sendMessage,
      sendDm,
      addPendingPlaceholder,
      markPendingFailed,
      dismissPendingMessage,
      showToast,
    ],
  );

  // Shared image extraction logic used by both the React onPaste handler
  // and the document-level fallback listener.
  const extractAndSendImage = useCallback(
    (clip: DataTransfer): boolean => {
      // Prefer DataTransferItemList (Chrome, Firefox).
      const items = clip.items;
      if (items?.length) {
        for (const item of items) {
          if (item.kind === "file" && item.type.startsWith("image/")) {
            const file = item.getAsFile();
            if (file) {
              sendMediaFile(file);
              return true;
            }
          }
        }
      }

      // Fallback: clipboardData.files (some engines only populate this).
      const files = clip.files;
      if (files?.length) {
        for (const file of files) {
          if (file.type.startsWith("image/")) {
            sendMediaFile(file);
            return true;
          }
        }
      }

      return false;
    },
    [sendMediaFile],
  );

  // Read image from clipboard via the async Clipboard API.  This is the
  // only reliable way to get pasted image data on WebKitGTK (Linux),
  // where the synchronous clipboardData on paste events is empty for
  // images.
  const readClipboardImage = useCallback(async (): Promise<boolean> => {
    if (!navigator.clipboard?.read) return false;
    try {
      const clipItems = await navigator.clipboard.read();
      for (const item of clipItems) {
        const imageType = item.types.find((t) => t.startsWith("image/"));
        if (imageType) {
          const blob = await item.getType(imageType);
          const file = new File([blob], "clipboard.png", { type: imageType });
          sendMediaFile(file);
          return true;
        }
      }
    } catch {
      // Permission denied or API unavailable - fall through silently.
    }
    return false;
  }, [sendMediaFile]);

  // Track whether the React onPaste handler already processed the event
  // so the document-level fallback doesn't double-fire.
  const pasteHandledRef = useRef(false);

  const handlePaste = useCallback(
    (e: ClipboardEvent) => {
      const clip = e.clipboardData;
      if (!clip) return;

      if (extractAndSendImage(clip)) {
        e.preventDefault();
        pasteHandledRef.current = true;
      }
      // If no image found, let the default paste into the text input happen.
    },
    [extractAndSendImage],
  );

  // Document-level paste listener.  On most engines the React onPaste
  // already handles images, but WebKitGTK on Linux does not populate
  // clipboardData with image files for <textarea> paste events.  In that
  // case we fall back to the async Clipboard API.
  useEffect(() => {
    const onDocPaste = (e: globalThis.ClipboardEvent) => {
      // Skip if the React handler already processed this event.
      if (pasteHandledRef.current) {
        pasteHandledRef.current = false;
        return;
      }

      // Try synchronous DataTransfer first.
      const clip = e.clipboardData;
      if (clip && extractAndSendImage(clip)) {
        e.preventDefault();
        return;
      }

      // Async fallback: read image via Clipboard API (WebKitGTK).
      readClipboardImage();
    };
    document.addEventListener("paste", onDocPaste);
    return () => document.removeEventListener("paste", onDocPaste);
  }, [extractAndSendImage, readClipboardImage]);

  const handleGifSelect = useCallback(
    async (url: string, alt: string) => {
      const html = `<img src="${url}" alt="${alt}" />`;
      if (isDmMode && selectedDmUser !== null) {
        await sendDm(selectedDmUser, html);
      } else if (selectedChannel !== null) {
        await sendMessage(selectedChannel, html);
      }
    },
    [isDmMode, selectedDmUser, selectedChannel, sendMessage, sendDm],
  );

  return { sending, handleSend, sendMediaFile, handlePaste, handleGifSelect };
}
