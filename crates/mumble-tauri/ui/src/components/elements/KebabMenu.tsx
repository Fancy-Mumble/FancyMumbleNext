import { useState, useRef, useEffect, useCallback } from "react";
import { createPortal } from "react-dom";
import KebabMenuIcon from "../../assets/icons/navigation/kebab-menu.svg?react";
import styles from "./KebabMenu.module.css";

export interface KebabMenuItem {
  readonly id: string;
  readonly label: string;
  readonly icon?: React.ReactNode;
  readonly active?: boolean;
  readonly disabled?: boolean;
  /** Show a red notification dot next to this item. */
  readonly badge?: boolean;
  /** Render the item with destructive styling (red). */
  readonly danger?: boolean;
  readonly onClick: () => void;
}

interface KebabMenuProps {
  readonly items: KebabMenuItem[];
  readonly ariaLabel?: string;
  /** Show a red notification dot on the trigger button. */
  readonly badge?: boolean;
}

export default function KebabMenu({ items, ariaLabel = "More options", badge }: KebabMenuProps) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const [menuPos, setMenuPos] = useState<{ top: number; right: number } | null>(null);

  const close = useCallback(() => setOpen(false), []);

  useEffect(() => {
    if (!open) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [open, close]);

  const handleToggle = useCallback(() => {
    setOpen((prev) => {
      if (!prev && triggerRef.current) {
        const rect = triggerRef.current.getBoundingClientRect();
        setMenuPos({
          top: rect.bottom + 4,
          right: window.innerWidth - rect.right,
        });
      }
      return !prev;
    });
  }, []);

  return (
    <div className={styles.wrapper}>
      <button
        ref={triggerRef}
        className={`${styles.trigger} ${open ? styles.triggerOpen : ""}`}
        onClick={handleToggle}
        aria-label={ariaLabel}
        title={ariaLabel}
      >
        <KebabMenuIcon width={18} height={18} />
        {badge && <span className={styles.badgeDot} />}
      </button>

      {open && createPortal(
        <>
          <button
            type="button"
            className={styles.backdrop}
            onClick={close}
            aria-label="Close menu"
          />
          <div
            className={styles.menu}
            role="menu"
            style={menuPos ? { top: menuPos.top, right: menuPos.right } : undefined}
          >
            {items.map((item) => (
              <button
                key={item.id}
                className={[
                  styles.menuItem,
                  item.active ? styles.menuItemActive : "",
                  item.danger ? styles.menuItemDanger : "",
                ].filter(Boolean).join(" ")}
                role="menuitem"
                disabled={item.disabled}
                onClick={() => {
                  item.onClick();
                  close();
                }}
              >
                {item.icon && <span className={styles.menuItemIcon}>{item.icon}</span>}
                {item.label}
                {item.badge && <span className={styles.itemBadgeDot} />}
              </button>
            ))}
          </div>
        </>,
        document.body
      )}
    </div>
  );
}
