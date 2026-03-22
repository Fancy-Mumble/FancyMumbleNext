import { useEffect, useRef, useState, useCallback } from "react";
import { createPortal } from "react-dom";
import type { ChatMessage } from "../types";
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
}

// -- Component ----------------------------------------------------

export default function MessageContextMenu({
  menu,
  canDelete,
  onClose,
  onDelete,
  onSelectMode,
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
        {canDelete && (
          <button type="button" className={`${styles.menuItem} ${styles.menuItemDanger}`} onClick={handleDelete}>
            <span className={styles.menuIcon}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="3 6 5 6 21 6" />
                <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
              </svg>
            </span>
            Delete message
          </button>
        )}
        {canDelete && (
          <button type="button" className={styles.menuItem} onClick={handleSelect}>
            <span className={styles.menuIcon}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="9 11 12 14 22 4" />
                <path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" />
              </svg>
            </span>
            Select messages
          </button>
        )}
      </div>
    </>,
    document.body,
  );
}
