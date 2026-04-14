import { useEffect, useRef, useState, useCallback, useMemo } from "react";
import { createPortal } from "react-dom";
import type { ChatMessage } from "../../types";
import type { ReactionSummary } from "./reactionStore";
import { getReadersForMessage } from "./readReceiptStore";
import { useAppStore } from "../../store";
import { QUICK_REACTIONS } from "../elements/MessageActionBar";
import EmojiPlusIcon from "../../assets/icons/communication/emoji-plus.svg?react";
import QuoteIcon from "../../assets/icons/communication/quote.svg?react";
import CopyIcon from "../../assets/icons/action/copy.svg?react";
import EditIcon from "../../assets/icons/action/edit.svg?react";
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
  readonly onEdit?: (msg: ChatMessage) => void;
  /** Reactions on the context-menu's target message. */
  readonly reactions?: readonly ReactionSummary[];
  /** Avatar data-URLs keyed by cert hash. */
  readonly avatarByHash?: ReadonlyMap<string, string>;
  /** Ordered message IDs for read-receipt watermark comparison. */
  readonly allMessageIds?: string[];
  /** Channel the message belongs to. */
  readonly channelId?: number;
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
  onEdit,
  reactions,
  avatarByHash,
  allMessageIds,
  channelId,
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

  // Build flat list of reactor entries from all reactions
  const reactorEntries = useMemo(() => {
    if (!reactions || reactions.length === 0) return [];
    const entries: { emoji: string; name: string; avatarUrl?: string }[] = [];
    for (const r of reactions) {
      for (const [hash, name] of r.reactorHashNames) {
        entries.push({ emoji: r.emoji, name, avatarUrl: avatarByHash?.get(hash) });
      }
    }
    return entries;
  }, [reactions, avatarByHash]);

  // Build list of users who read this message
  const readReceiptVersion = useAppStore((s) => s.readReceiptVersion);
  const ownSession = useAppStore((s) => s.ownSession);
  const users = useAppStore((s) => s.users);
  const ownHash = useMemo(() => users.find((u) => u.session === ownSession)?.hash, [users, ownSession]);

  const isOwnWithId = menu.message.is_own && !!menu.message.message_id && channelId != null && !!allMessageIds;

  const readerEntries = useMemo(() => {
    const msgId = menu.message.message_id;
    if (!msgId || !menu.message.is_own || channelId == null || !allMessageIds) return [];
    const readers = getReadersForMessage(channelId, msgId, allMessageIds);
    return readers
      .filter((r) => r.cert_hash !== ownHash)
      .map((r) => ({ name: r.name, avatarUrl: avatarByHash?.get(r.cert_hash) }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [menu.message, channelId, allMessageIds, avatarByHash, ownHash, readReceiptVersion]);

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
        {onEdit && menu.message.is_own && menu.message.message_id && (
          <button type="button" className={styles.menuItem} onClick={() => { onEdit(menu.message); onClose(); }}>
            <span className={styles.menuIcon}>
              <EditIcon width={14} height={14} />
            </span>
            Edit message
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

        {/* Reactor list */}
        {reactorEntries.length > 0 && (
          <>
            <div className={styles.divider} />
            <div className={styles.reactorSection}>
              {reactorEntries.map((entry) => (
                <div key={`${entry.emoji}-${entry.name}`} className={styles.reactorItem}>
                  <span className={styles.reactorEmoji}>{entry.emoji}</span>
                  {entry.avatarUrl ? (
                    <img src={entry.avatarUrl} alt="" className={styles.reactorAvatar} />
                  ) : (
                    <div className={styles.reactorAvatarFallback}>
                      {entry.name.charAt(0).toUpperCase()}
                    </div>
                  )}
                  <span className={styles.reactorName}>{entry.name}</span>
                </div>
              ))}
            </div>
          </>
        )}

        {/* Read by list */}
        {isOwnWithId && (
          <>
            <div className={styles.divider} />
            <div className={styles.readByLabel}>Read by</div>
            {readerEntries.length > 0 ? (
              <div className={styles.reactorSection}>
                {readerEntries.map((entry) => (
                  <div key={entry.name} className={styles.reactorItem}>
                    {entry.avatarUrl ? (
                      <img src={entry.avatarUrl} alt="" className={styles.reactorAvatar} />
                    ) : (
                      <div className={styles.reactorAvatarFallback}>
                        {entry.name.charAt(0).toUpperCase()}
                      </div>
                    )}
                    <span className={styles.reactorName}>{entry.name}</span>
                  </div>
                ))}
              </div>
            ) : (
              <div className={styles.readByEmpty}>No one yet</div>
            )}
          </>
        )}
      </div>
    </>,
    document.body,
  );
}
