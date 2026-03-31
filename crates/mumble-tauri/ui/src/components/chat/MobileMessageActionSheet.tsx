import { useCallback, useMemo } from "react";
import type { ChatMessage } from "../../types";
import { QUICK_REACTIONS } from "../elements/MessageActionBar";
import MobileBottomSheet from "../elements/MobileBottomSheet";
import EmojiPlusIcon from "../../assets/icons/communication/emoji-plus.svg?react";
import QuoteIcon from "../../assets/icons/communication/quote.svg?react";
import CopyIcon from "../../assets/icons/action/copy.svg?react";
import TrashIcon from "../../assets/icons/action/trash.svg?react";
import CheckboxIcon from "../../assets/icons/status/checkbox.svg?react";
import styles from "./MobileMessageActionSheet.module.css";

const MAX_PREVIEW_LEN = 200;

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

function extractFirstImageSrc(html: string): string | null {
  const match = /<img[^>]+src="([^"]+)"/i.exec(html);
  return match?.[1] ?? null;
}

interface MobileMessageActionSheetProps {
  readonly message: ChatMessage;
  readonly canDelete: boolean;
  readonly onClose: () => void;
  readonly onDelete: (msg: ChatMessage) => void;
  readonly onSelectMode: (msg: ChatMessage) => void;
  readonly onReaction?: (msg: ChatMessage, emoji: string) => void;
  readonly onMoreReactions?: (msg: ChatMessage, e?: React.MouseEvent) => void;
  readonly onCite?: (msg: ChatMessage) => void;
  readonly onCopyText?: (msg: ChatMessage) => void;
}

export default function MobileMessageActionSheet({
  message,
  canDelete,
  onClose,
  onDelete,
  onSelectMode,
  onReaction,
  onMoreReactions,
  onCite,
  onCopyText,
}: MobileMessageActionSheetProps) {
  const previewText = useMemo(() => {
    const text = stripHtml(message.body);
    if (text.length <= MAX_PREVIEW_LEN) return text;
    return text.slice(0, MAX_PREVIEW_LEN) + "\u2026";
  }, [message.body]);

  const previewImage = useMemo(() => extractFirstImageSrc(message.body), [message.body]);
  const hasTextPreview = previewText.length > 0;

  const act = useCallback(
    (fn: (msg: ChatMessage) => void) => () => {
      fn(message);
      onClose();
    },
    [message, onClose],
  );

  const actEmoji = useCallback(
    (emoji: string) => () => {
      onReaction?.(message, emoji);
      onClose();
    },
    [message, onReaction, onClose],
  );

  return (
    <MobileBottomSheet open onClose={onClose} ariaLabel="Close message actions">
      {/* Message preview */}
      {(hasTextPreview || previewImage) && (
        <div className={styles.preview}>
          {previewImage && (
            <img
              className={styles.previewImage}
              src={previewImage}
              alt=""
              draggable={false}
            />
          )}
          {hasTextPreview && (
            <p className={styles.previewText}>{previewText}</p>
          )}
        </div>
      )}

      {/* Quick reactions */}
      {onReaction && (
        <div className={styles.reactionRow}>
          {QUICK_REACTIONS.map((r) => (
            <button
              key={r.label}
              type="button"
              className={styles.reactionBtn}
              aria-label={r.label}
              onClick={actEmoji(r.emoji)}
            >
              {r.emoji}
            </button>
          ))}
          {onMoreReactions && (
            <button
              type="button"
              className={styles.reactionBtn}
              aria-label="More reactions"
              onClick={act((m) => onMoreReactions(m))}
            >
              <EmojiPlusIcon width={18} height={18} />
            </button>
          )}
        </div>
      )}

      {/* Actions */}
      <div className={styles.actions}>
        {onCite && (
          <button type="button" className={styles.actionItem} onClick={act((m) => onCite(m))}>
            <span className={styles.actionIcon}>
              <QuoteIcon width={16} height={16} />
            </span>
            Quote
          </button>
        )}
        {onCopyText && (
          <button type="button" className={styles.actionItem} onClick={act((m) => onCopyText(m))}>
            <span className={styles.actionIcon}>
              <CopyIcon width={16} height={16} />
            </span>
            Copy text
          </button>
        )}
        {canDelete && (
          <>
            <button
              type="button"
              className={`${styles.actionItem} ${styles.actionItemDanger}`}
              onClick={act(onDelete)}
            >
              <span className={styles.actionIcon}>
                <TrashIcon width={16} height={16} />
              </span>
              Delete message
            </button>
            <button type="button" className={styles.actionItem} onClick={act(onSelectMode)}>
              <span className={styles.actionIcon}>
                <CheckboxIcon width={16} height={16} />
              </span>
              Select messages
            </button>
          </>
        )}
      </div>
    </MobileBottomSheet>
  );
}
