import { useEffect, useRef, useState, useCallback } from "react";
import { createPortal } from "react-dom";
import type { ChatMessage } from "../../types";
import { QUICK_REACTIONS } from "../elements/MessageActionBar";
import EmojiPlusIcon from "../../assets/icons/communication/emoji-plus.svg?react";
import QuoteIcon from "../../assets/icons/communication/quote.svg?react";
import CopyIcon from "../../assets/icons/action/copy.svg?react";
import TrashIcon from "../../assets/icons/action/trash.svg?react";
import CheckboxIcon from "../../assets/icons/status/checkbox.svg?react";
import styles from "./MessageContextMenu.module.css";

// -- Overflow-aware position computation --------------------------

interface MenuPosition { top: number; left: number }

function computePosition(
  clickX: number,
  clickY: number,
  menuEl: HTMLElement,
): MenuPosition {
  const { innerWidth: vw, innerHeight: vh } = window;
  const { width: w, height: h } = menuEl.getBoundingClientRect();
  const margin = 4;

  let left = clickX;
  let top = clickY;

  if (left + w + margin > vw) left = Math.max(margin, clickX - w);
  if (top + h + margin > vh) top = Math.max(margin, clickY - h);

  return { top, left };
}

// -- Public types -------------------------------------------------

export interface MessageContextMenuState {
  x: number;
  y: number;
  message: ChatMessage;
}

interface MessageContextMenuProps {
  readonly menu: MessageContextMenuState;
  readonly canDelete: boolean;
  readonly onClose: () => void;
  readonly onDelete: (msg: ChatMessage) => void;
  readonly onSelectMode: (msg: ChatMessage) => void;
  readonly onReaction?: (msg: ChatMessage, emoji: string) => void;
  readonly onMoreReactions?: (msg: ChatMessage, e?: React.MouseEvent) => void;
  readonly onCite?: (msg: ChatMessage) => void;
  readonly onCopyText?: (msg: ChatMessage) => void;
}

// -- Component ----------------------------------------------------

export default function MessageContextMenu({
  menu,
  canDelete,
  onClose,
  onDelete,
  onSelectMode,
  onReaction,
  onMoreReactions,
  onCite,
  onCopyText,
}: MessageContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<MenuPosition | null>(null);

  useEffect(() => {
    if (menuRef.current) {
      setPos(computePosition(menu.x, menu.y, menuRef.current));
    }
  }, [menu.x, menu.y]);

  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [onClose]);

  const handleDelete = useCallback(() => {
    onDelete(menu.message);
    onClose();
  }, [menu.message, onDelete, onClose]);

  const handleSelect = useCallback(() => {
    onSelectMode(menu.message);
    onClose();
  }, [menu.message, onSelectMode, onClose]);

  return createPortal(
    <>
      <div className={styles.overlay} onClick={onClose} onContextMenu={(e) => { e.preventDefault(); onClose(); }} />
      <div
        ref={menuRef}
        className={styles.menu}
        style={pos ? { top: pos.top, left: pos.left } : { top: menu.y, left: menu.x, visibility: "hidden" }}
      >
        {/* Quick-reaction row */}
        {onReaction && (
          <div className={styles.reactionRow}>
            {QUICK_REACTIONS.map((r) => (
              <button
                key={r.label}
                type="button"
                className={styles.reactionBtn}
                aria-label={r.label}
                onClick={() => { onReaction(menu.message, r.emoji); onClose(); }}
              >
                {r.emoji}
              </button>
            ))}
            {onMoreReactions && (
              <button
                type="button"
                className={styles.reactionBtn}
                aria-label="More reactions"
                onClick={(e) => { onMoreReactions(menu.message, e); onClose(); }}
              >
                <EmojiPlusIcon width={16} height={16} />
              </button>
            )}
          </div>
        )}
        {onReaction && <div className={styles.divider} />}
        {onCite && (
          <button type="button" className={styles.menuItem} onClick={() => { onCite(menu.message); onClose(); }}>
            <span className={styles.menuIcon}>
              <QuoteIcon width={14} height={14} />
            </span>
            Quote
          </button>
        )}
        {onCopyText && (
          <button type="button" className={styles.menuItem} onClick={() => { onCopyText(menu.message); onClose(); }}>
            <span className={styles.menuIcon}>
              <CopyIcon width={14} height={14} />
            </span>
            Copy text
          </button>
        )}
        {canDelete && (
          <button type="button" className={`${styles.menuItem} ${styles.menuItemDanger}`} onClick={handleDelete}>
            <span className={styles.menuIcon}>
              <TrashIcon width={14} height={14} />
            </span>
            Delete message
          </button>
        )}
        {canDelete && (
          <button type="button" className={styles.menuItem} onClick={handleSelect}>
            <span className={styles.menuIcon}>
              <CheckboxIcon width={14} height={14} />
            </span>
            Select messages
          </button>
        )}
      </div>
    </>,
    document.body,
  );
}
