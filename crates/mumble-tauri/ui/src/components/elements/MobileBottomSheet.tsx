/**
 * MobileBottomSheet - a reusable bottom-sheet overlay that slides up
 * from the bottom of the screen.  Supports swipe-down-to-dismiss.
 *
 * Used by MobileProfileSheet (user cards) and ServerInfoPanel on mobile.
 */

import { useCallback, useEffect, useRef, type ReactNode } from "react";
import styles from "./MobileBottomSheet.module.css";

interface MobileBottomSheetProps {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly ariaLabel?: string;
  readonly children: ReactNode;
}

export default function MobileBottomSheet({
  open,
  onClose,
  ariaLabel = "Close",
  children,
}: MobileBottomSheetProps) {
  const sheetRef = useRef<HTMLDivElement>(null);

  const dismiss = useCallback(() => onClose(), [onClose]);

  // Close on Escape key.
  useEffect(() => {
    if (!open) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") dismiss();
    };
    globalThis.addEventListener("keydown", handleKey);
    return () => globalThis.removeEventListener("keydown", handleKey);
  }, [open, dismiss]);

  // Swipe-down-to-dismiss.
  useEffect(() => {
    if (!open) return;
    const el = sheetRef.current;
    if (!el) return;

    let startY = 0;
    let dragging = false;

    const onStart = (e: TouchEvent) => {
      if (el.scrollTop > 0) return;
      const touch = e.touches[0];
      if (!touch) return;
      startY = touch.clientY;
      dragging = true;
    };

    const onMove = (e: TouchEvent) => {
      if (!dragging) return;
      const touch = e.touches[0];
      if (!touch) return;
      const dy = touch.clientY - startY;
      if (dy < 0) {
        el.style.transition = "none";
        el.style.transform = "";
        return;
      }
      el.style.transition = "none";
      el.style.transform = `translateY(${dy}px)`;
    };

    const onEnd = (e: TouchEvent) => {
      if (!dragging) return;
      dragging = false;
      const touch = e.changedTouches[0];
      if (!touch) {
        el.style.transition = "";
        el.style.transform = "";
        return;
      }
      const dy = touch.clientY - startY;
      el.style.transition = "";
      el.style.transform = "";
      if (dy > 100) {
        dismiss();
      }
    };

    el.addEventListener("touchstart", onStart, { passive: true });
    el.addEventListener("touchmove", onMove, { passive: true });
    el.addEventListener("touchend", onEnd, { passive: true });

    return () => {
      el.removeEventListener("touchstart", onStart);
      el.removeEventListener("touchmove", onMove);
      el.removeEventListener("touchend", onEnd);
    };
  }, [open, dismiss]);

  if (!open) return null;

  return (
    <div className={styles.overlay}>
      <button
        className={styles.backdropBtn}
        onClick={dismiss}
        aria-label={ariaLabel}
        type="button"
      />
      <div ref={sheetRef} className={styles.sheet}>
        <div className={styles.handle}>
          <div className={styles.handleBar} />
        </div>
        <div className={styles.content}>{children}</div>
      </div>
    </div>
  );
}
