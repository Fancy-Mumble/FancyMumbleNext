import { useCallback, useMemo } from "react";
import type { ChatMessage } from "../../types";
import type { ReactionSummary } from "./reactionStore";
import { getReadersForMessage } from "./readReceiptStore";
import { useAppStore } from "../../store";
import { QUICK_REACTIONS } from "../elements/MessageActionBar";
import MobileBottomSheet from "../elements/MobileBottomSheet";
import EmojiPlusIcon from "../../assets/icons/communication/emoji-plus.svg?react";
import QuoteIcon from "../../assets/icons/communication/quote.svg?react";
import CopyIcon from "../../assets/icons/action/copy.svg?react";
import EditIcon from "../../assets/icons/action/edit.svg?react";
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
  readonly onEdit?: (msg: ChatMessage) => void;
  /** Pin or unpin a message. */
  readonly onPin?: (msg: ChatMessage) => void;
  /** Existing reactions on this message (for showing reactor names on mobile). */
  readonly reactions?: readonly ReactionSummary[];
  /** Ordered message IDs for read-receipt watermark comparison. */
  readonly allMessageIds?: string[];
  /** Channel the message belongs to. */
  readonly channelId?: number;
  /** Avatar data-URLs keyed by cert hash. */
  readonly avatarByHash?: ReadonlyMap<string, string>;
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
  onEdit,
  onPin,
  reactions,
  allMessageIds,
  channelId,
  avatarByHash,
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

  // Build list of users who read this message
  const readReceiptVersion = useAppStore((s) => s.readReceiptVersion);
  const ownSession = useAppStore((s) => s.ownSession);
  const users = useAppStore((s) => s.users);
  const ownHash = useMemo(() => users.find((u) => u.session === ownSession)?.hash, [users, ownSession]);

  const isOwnWithId = message.is_own && !!message.message_id && channelId != null && !!allMessageIds;

  const readerEntries = useMemo(() => {
    const msgId = message.message_id;
    if (!msgId || !message.is_own || channelId == null || !allMessageIds) return [];
    const readers = getReadersForMessage(channelId, msgId, allMessageIds);
    return readers
      .filter((r) => r.name && (!ownHash || r.cert_hash !== ownHash))
      .map((r) => ({ certHash: r.cert_hash, name: r.name, isOnline: r.is_online, avatarUrl: avatarByHash?.get(r.cert_hash) }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [message, channelId, allMessageIds, avatarByHash, ownHash, readReceiptVersion]);

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

      {/* Existing reactions with reactor names (mobile-specific) */}
      {reactions && reactions.length > 0 && (
        <div className={styles.reactorList}>
          {reactions.map((r) => {
            const unique = [...new Set(r.reactorHashNames.values())];
            if (unique.length === 0) return null;
            return (
              <div key={r.emoji} className={styles.reactorRow}>
                <span className={styles.reactorEmoji}>{r.emoji}</span>
                <span className={styles.reactorNames}>{unique.join(", ")}</span>
              </div>
            );
          })}
        </div>
      )}

      {/* Read by list */}
      {isOwnWithId && (
        <div className={styles.reactorList}>
          <div className={styles.readByLabel}>Read by</div>
          {readerEntries.length > 0 ? (
            readerEntries.map((entry) => (
              <div key={entry.certHash} className={`${styles.readByRow} ${entry.isOnline ? "" : styles.offlineReader}`}>
                {entry.avatarUrl ? (
                  <img src={entry.avatarUrl} alt="" className={styles.readByAvatar} />
                ) : (
                  <div className={styles.readByAvatarFallback}>
                    {entry.name.charAt(0).toUpperCase()}
                  </div>
                )}
                <span className={styles.reactorNames}>{entry.name}</span>
              </div>
            ))
          ) : (
            <div className={styles.readByEmpty}>No one yet</div>
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
        {onEdit && message.is_own && message.message_id && (
          <button type="button" className={styles.actionItem} onClick={act((m) => onEdit(m))}>
            <span className={styles.actionIcon}>
              <EditIcon width={16} height={16} />
            </span>
            Edit message
          </button>
        )}
        {onPin && message.message_id && (
          <button type="button" className={styles.actionItem} onClick={act((m) => onPin(m))}>
            <span className={styles.actionIcon}>📌</span>
            {message.pinned ? "Unpin message" : "Pin message"}
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
