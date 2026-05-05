import { CopyIcon, EmojiPlusIcon, KebabMenuIcon, PlayIcon, QuoteIcon, TrashIcon } from "../../icons";
import { useState, useCallback, useRef, useEffect } from "react";
import { createPortal } from "react-dom";
import type { ChatMessage } from "../../types";
import { useWatchStart } from "../chat/watch/useWatchStart";
import styles from "./MessageActionBar.module.css";

// -- Quick reactions shown as emoji buttons -----------------------

export const QUICK_REACTIONS = [
  { emoji: "\uD83D\uDC4D", label: "Like" },
  { emoji: "\u2764\uFE0F",  label: "Heart" },
  { emoji: "\uD83D\uDE02", label: "Laugh" },
  { emoji: "\uD83D\uDE2E", label: "Surprise" },
] as const;

// -- Kebab menu items (extendable) --------------------------------

interface KebabEntry {
  readonly id: string;
  readonly label: string;
  readonly icon: React.ReactNode;
  readonly danger?: boolean;
  readonly onClick: () => void;
}

// -- Public props -------------------------------------------------

export interface MessageActionBarProps {
  readonly message: ChatMessage;
  readonly isOwn: boolean;
  /** Called when a quick-reaction emoji is clicked. */
  readonly onReaction: (message: ChatMessage, emoji: string) => void;
  /** Called when the "more reactions" button is clicked. */
  readonly onMoreReactions: (message: ChatMessage, e?: React.MouseEvent) => void;
  /** Called when the cite/quote button is clicked. */
  readonly onCite: (message: ChatMessage) => void;
  /** Called when the user chooses "Copy text" from the kebab menu. */
  readonly onCopyText?: (message: ChatMessage) => void;
  /** Called when the user chooses "Delete" from the kebab menu. */
  readonly onDelete?: (message: ChatMessage) => void;
  /** Whether the current user can delete this message. */
  readonly canDelete?: boolean;
}

// -- Component ----------------------------------------------------

export default function MessageActionBar({
  message,
  isOwn,
  onReaction,
  onMoreReactions,
  onCite,
  onCopyText,
  onDelete,
  canDelete = false,
}: MessageActionBarProps) {
  const { canStart: canWatchTogether, busy: watchBusy, start: startWatch } = useWatchStart(
    message.body,
    message.channel_id,
  );
  const [kebabOpen, setKebabOpen] = useState(false);
  const kebabBtnRef = useRef<HTMLButtonElement>(null);
  const [kebabPos, setKebabPos] = useState<{ top: number; right: number } | null>(null);

  const closeKebab = useCallback(() => setKebabOpen(false), []);

  // Close on Escape
  useEffect(() => {
    if (!kebabOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeKebab();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [kebabOpen, closeKebab]);

  const toggleKebab = useCallback(() => {
    setKebabOpen((prev) => {
      if (!prev && kebabBtnRef.current) {
        const rect = kebabBtnRef.current.getBoundingClientRect();
        setKebabPos({
          top: rect.bottom + 4,
          right: window.innerWidth - rect.right,
        });
      }
      return !prev;
    });
  }, []);

  // Build kebab menu entries
  const kebabItems: KebabEntry[] = [];
  if (canWatchTogether) {
    kebabItems.push({
      id: "watch-together",
      label: watchBusy ? "Starting\u2026" : "Watch together",
      icon: <PlayIcon width={14} height={14} />,
      onClick: () => { void startWatch(); },
    });
  }
  if (canDelete && onDelete) {
    kebabItems.push({
      id: "delete",
      label: "Delete",
      icon: <TrashIcon width={14} height={14} />,
      danger: true,
      onClick: () => onDelete(message),
    });
  }

  return (
    <div
      className={`${styles.actionBar} ${isOwn ? "" : styles.actionBarOwn}`}
      data-action-bar=""
    >
      {/* Quick-reaction emoji buttons */}
      {QUICK_REACTIONS.map((r) => (
        <button
          key={r.label}
          type="button"
          className={styles.actionBtn}
          title={r.label}
          aria-label={r.label}
          onClick={() => onReaction(message, r.emoji)}
        >
          {r.emoji}
        </button>
      ))}
      {/* More reactions */}
      <button
        type="button"
        className={styles.actionBtn}
        title="More reactions"
        aria-label="More reactions"
        onClick={(e) => onMoreReactions(message, e)}
      >
        <EmojiPlusIcon width={16} height={16} />
      </button>

      {/* Separator */}
      <div className={styles.separator} aria-hidden="true" />

      {/* Cite / quote */}
      <button
        type="button"
        className={styles.actionBtn}
        title="Quote"
        aria-label="Quote message"
        onClick={() => onCite(message)}
      >
        <QuoteIcon width={16} height={16} />
      </button>

      {/* Copy text */}
      {onCopyText && (
        <button
          type="button"
          className={styles.actionBtn}
          title="Copy text"
          aria-label="Copy message text"
          onClick={() => onCopyText(message)}
        >
          <CopyIcon width={16} height={16} />
        </button>
      )}

      {/* Kebab menu */}
      {kebabItems.length > 0 && (
        <div className={styles.kebabMenu}>
          <button
            ref={kebabBtnRef}
            type="button"
            className={styles.actionBtn}
            title="More options"
            aria-label="More options"
            onClick={toggleKebab}
          >
            <KebabMenuIcon width={16} height={16} />
          </button>

          {kebabOpen && createPortal(
            <>
              <div className={styles.kebabBackdrop} onClick={closeKebab} />
              <div
                className={styles.kebabDropdown}
                role="menu"
                style={kebabPos ? { top: kebabPos.top, right: kebabPos.right, position: "fixed" } : undefined}
              >
                {kebabItems.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    className={styles.kebabItem}
                    role="menuitem"
                    onClick={() => {
                      item.onClick();
                      closeKebab();
                    }}
                  >
                    <span className={styles.kebabItemIcon}>{item.icon}</span>
                    {item.label}
                  </button>
                ))}
              </div>
            </>,
            document.body,
          )}
        </div>
      )}
    </div>
  );
}
