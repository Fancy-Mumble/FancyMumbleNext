import { useCallback, useMemo } from "react";
import { useAppStore } from "../../store";
import type { ChatMessage } from "../../types";
import { colorFor, formatTimestamp } from "../../utils/format";
import styles from "./QuoteBlock.module.css";

interface QuoteBlockProps {
  readonly messageId: string;
  readonly onScrollTo?: (messageId: string) => void;
}

/** Decode common HTML entities back to their characters. */
function decodeEntities(text: string): string {
  return text
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&amp;", "&");
}

/** Strip HTML tags and comment markers, decode entities, then truncate. */
function previewText(html: string, maxLen = 120): string {
  const text = decodeEntities(
    html
      .replaceAll(/<!--[\s\S]*?-->/g, "")
      .replaceAll(/<[^>]*>/g, "")
      .trim(),
  );
  if (text.length <= maxLen) return text;
  return text.slice(0, maxLen) + "\u2026";
}

/** Extract the first image or video poster src from an HTML body. */
function extractThumbnailSrc(html: string): string | null {
  const imgMatch = /<img[^>]+src="([^"]+)"/i.exec(html);
  if (imgMatch) return imgMatch[1];
  const vidMatch = /<video[^>]+src="([^"]+)"/i.exec(html);
  if (vidMatch) return vidMatch[1];
  const sourceMatch = /<source[^>]+src="([^"]+)"/i.exec(html);
  return sourceMatch ? sourceMatch[1] : null;
}

/** Find a message by ID across all message lists in the store. */
function findMessage(
  messageId: string,
  messages: ChatMessage[],
  dmMessages: ChatMessage[],
): ChatMessage | undefined {
  return (
    messages.find((m) => m.message_id === messageId) ??
    dmMessages.find((m) => m.message_id === messageId)
  );
}

export default function QuoteBlock({ messageId, onScrollTo }: QuoteBlockProps) {
  const messages = useAppStore((s) => s.messages);
  const dmMessages = useAppStore((s) => s.dmMessages);

  const quoted = useMemo(
    () => findMessage(messageId, messages, dmMessages),
    [messageId, messages, dmMessages],
  );

  const handleClick = useCallback(() => {
    onScrollTo?.(messageId);
  }, [messageId, onScrollTo]);

  if (!quoted) {
    return (
      <div className={styles.quoteBlock}>
        <div className={styles.quoteBar} />
        <div className={styles.quoteContent}>
          <span className={styles.quoteUnavailable}>Message unavailable</span>
        </div>
      </div>
    );
  }

  const preview = previewText(quoted.body);
  const hasMedia = /<img|<video/i.test(quoted.body);
  const thumbnailSrc = hasMedia ? extractThumbnailSrc(quoted.body) : null;

  return (
    <button
      type="button"
      className={styles.quoteBlock}
      onClick={handleClick}
      title="Click to scroll to message"
    >
      <div
        className={styles.quoteBar}
        style={{ backgroundColor: colorFor(quoted.sender_name) }}
      />
      <div className={styles.quoteContent}>
        <span
          className={styles.quoteSender}
          style={{ color: colorFor(quoted.sender_name) }}
        >
          {quoted.sender_name}
          {quoted.timestamp != null && (
            <time
              className={styles.quoteTimestamp}
              dateTime={new Date(quoted.timestamp).toISOString()}
            >
              {formatTimestamp(quoted.timestamp)}
            </time>
          )}
        </span>
        <span className={styles.quoteText}>
          {preview || (hasMedia ? "Photo" : "Empty message")}
        </span>
      </div>
      {thumbnailSrc && (
        <img
          src={thumbnailSrc}
          alt=""
          className={styles.quoteThumbnail}
          draggable={false}
        />
      )}
    </button>
  );
}
