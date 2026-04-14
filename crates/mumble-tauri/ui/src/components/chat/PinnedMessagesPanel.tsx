import { useMemo } from "react";
import type { ChatMessage } from "../../types";
import CloseIcon from "../../assets/icons/action/close.svg?react";
import styles from "./PinnedMessagesPanel.module.css";

const MAX_PREVIEW = 120;

function stripHtml(html: string): string {
  return html
    .replaceAll(/<!--[\s\S]*?-->/g, "")
    .replaceAll(/<br\s*\/?>/gi, "\n")
    .replaceAll(/<[^>]*>/g, "")
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&amp;", "&")
    .trim();
}

function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  return text.slice(0, max) + "\u2026";
}

interface PinnedMessagesPanelProps {
  readonly messages: readonly ChatMessage[];
  readonly unseenIds: ReadonlySet<string>;
  readonly onClose: () => void;
  readonly onNavigate: (messageId: string) => void;
  readonly onUnpin?: (msg: ChatMessage) => void;
}

export default function PinnedMessagesPanel({
  messages,
  unseenIds,
  onClose,
  onNavigate,
  onUnpin,
}: PinnedMessagesPanelProps) {
  const pinnedMessages = useMemo(
    () => messages.filter((m) => m.pinned),
    [messages],
  );

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <span className={styles.title}>
          📌 Pinned Messages
          {pinnedMessages.length > 0 && (
            <span className={styles.count}>{pinnedMessages.length}</span>
          )}
        </span>
        <button
          type="button"
          className={styles.closeBtn}
          onClick={onClose}
          aria-label="Close pinned messages"
        >
          <CloseIcon width={16} height={16} />
        </button>
      </div>

      {pinnedMessages.length === 0 ? (
        <div className={styles.empty}>No pinned messages in this channel.</div>
      ) : (
        <div className={styles.list}>
          {pinnedMessages.map((msg) => {
            const id = msg.message_id ?? "";
            const preview = truncate(stripHtml(msg.body), MAX_PREVIEW);
            const isUnseen = unseenIds.has(id);

            return (
              <button
                key={id}
                type="button"
                className={styles.item}
                onClick={() => {
                  onNavigate(id);
                  onClose();
                }}
              >
                <div className={styles.itemHeader}>
                  <span className={styles.senderName}>{msg.sender_name}</span>
                  {isUnseen && <span className={styles.unseenDot} />}
                  {msg.pinned_by && (
                    <span className={styles.pinnedBy}>
                      pinned by {msg.pinned_by}
                    </span>
                  )}
                </div>
                <div className={styles.preview}>{preview || "(media)"}</div>
                {onUnpin && (
                  <button
                    type="button"
                    className={styles.unpinBtn}
                    onClick={(e) => {
                      e.stopPropagation();
                      onUnpin(msg);
                    }}
                    aria-label="Unpin message"
                  >
                    Unpin
                  </button>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
