import { useRef, useCallback, useEffect, useState } from "react";
import styles from "./SwipeableCard.module.css";

interface SwipeAction {
  label: string;
  icon?: string;
  color: string;
  onTrigger: () => void;
}

interface Props {
  /** Content rendered inside the swipeable surface. */
  children: React.ReactNode;
  /** Action revealed when swiping left (card moves left, action on the right). */
  leftSwipeAction?: SwipeAction;
  /** Action revealed when swiping right (card moves right, action on the left). */
  rightSwipeAction?: SwipeAction;
  /** Minimum horizontal distance (px) to trigger the action. Default 80. */
  threshold?: number;
  /** Extra className applied to the outer wrapper. */
  className?: string;
  /** Whether swipe is disabled (e.g. during connection). */
  disabled?: boolean;
}

const DEFAULT_THRESHOLD = 120;
/** Minimum horizontal movement before we lock into swipe mode. */
const LOCK_THRESHOLD = 10;

/**
 * Wraps any card-like element and adds horizontal swipe-to-reveal actions
 * on touch devices. On desktop (no touch) rendering is transparent.
 */
export default function SwipeableCard({
  children,
  leftSwipeAction,
  rightSwipeAction,
  threshold = DEFAULT_THRESHOLD,
  className,
  disabled,
}: Props) {
  const innerRef = useRef<HTMLDivElement>(null);
  const startX = useRef(0);
  const startY = useRef(0);
  const currentX = useRef(0);
  /** Whether we have decided this gesture is a horizontal swipe. */
  const locked = useRef<"horizontal" | "vertical" | null>(null);
  const [offset, setOffset] = useState(0);
  const [settling, setSettling] = useState(false);
  const [triggered, setTriggered] = useState<"left" | "right" | null>(null);

  const reset = useCallback(() => {
    setSettling(true);
    setOffset(0);
    // Wait for the CSS transition to finish before clearing the settling flag.
    const id = setTimeout(() => {
      setSettling(false);
      setTriggered(null);
    }, 250);
    return () => clearTimeout(id);
  }, []);

  useEffect(() => {
    const el = innerRef.current;
    if (!el) return;

    const onTouchStart = (e: TouchEvent) => {
      if (disabled) return;
      const touch = e.touches[0];
      if (!touch) return;
      startX.current = touch.clientX;
      startY.current = touch.clientY;
      currentX.current = touch.clientX;
      locked.current = null;
      setSettling(false);
      setTriggered(null);
    };

    const onTouchMove = (e: TouchEvent) => {
      if (disabled) return;
      const touch = e.touches[0];
      if (!touch) return;

      const dx = touch.clientX - startX.current;
      const dy = touch.clientY - startY.current;

      // Decide direction lock on first meaningful movement
      if (locked.current === null) {
        if (Math.abs(dx) > LOCK_THRESHOLD || Math.abs(dy) > LOCK_THRESHOLD) {
          locked.current = Math.abs(dx) > Math.abs(dy) ? "horizontal" : "vertical";
        }
        if (locked.current !== "horizontal") return;
      }
      if (locked.current !== "horizontal") return;

      // Prevent vertical scrolling while swiping horizontally
      e.preventDefault();

      currentX.current = touch.clientX;

      // Clamp: only allow directions that have an action
      let clamped = dx;
      if (clamped < 0 && !leftSwipeAction) clamped = 0;
      if (clamped > 0 && !rightSwipeAction) clamped = 0;

      // Apply rubber-band resistance past the threshold
      const sign = Math.sign(clamped);
      const abs = Math.abs(clamped);
      const dampened =
        abs <= threshold ? abs : threshold + (abs - threshold) * 0.3;
      setOffset(sign * dampened);
    };

    const onTouchEnd = () => {
      if (disabled || locked.current !== "horizontal") {
        locked.current = null;
        return;
      }
      locked.current = null;

      const dx = currentX.current - startX.current;

      if (dx < -threshold && leftSwipeAction) {
        setTriggered("left");
        // Animate briefly then fire
        setSettling(true);
        setOffset(0);
        setTimeout(() => {
          leftSwipeAction.onTrigger();
          setSettling(false);
          setTriggered(null);
        }, 200);
      } else if (dx > threshold && rightSwipeAction) {
        setTriggered("right");
        setSettling(true);
        setOffset(0);
        setTimeout(() => {
          rightSwipeAction.onTrigger();
          setSettling(false);
          setTriggered(null);
        }, 200);
      } else {
        reset();
      }
    };

    el.addEventListener("touchstart", onTouchStart, { passive: true });
    // touchmove is NOT passive so we can preventDefault to stop scroll
    el.addEventListener("touchmove", onTouchMove, { passive: false });
    el.addEventListener("touchend", onTouchEnd, { passive: true });
    el.addEventListener("touchcancel", () => reset(), { passive: true });

    return () => {
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("touchmove", onTouchMove);
      el.removeEventListener("touchend", onTouchEnd);
      el.removeEventListener("touchcancel", () => reset());
    };
  }, [
    disabled,
    leftSwipeAction,
    rightSwipeAction,
    threshold,
    reset,
  ]);

  const showLeft = offset < 0 || triggered === "left";
  const showRight = offset > 0 || triggered === "right";

  // 0 → 1 progress toward the trigger threshold (capped at 1)
  const progress = Math.min(Math.abs(offset) / threshold, 1);
  // Label fades in during the second half of the swipe
  const labelOpacity = Math.max(0, (progress - 0.4) / 0.6);

  return (
    <div
      className={`${styles.wrapper} ${className ?? ""}`}
    >
      {/* Left-swipe action (appears on the right) */}
      {leftSwipeAction && showLeft && (
        <div
          className={styles.actionRight}
          style={{ background: leftSwipeAction.color, opacity: progress }}
        >
          <span className={styles.actionLabel} style={{ opacity: labelOpacity }}>
            {leftSwipeAction.icon && (
              <span className={styles.actionIcon}>{leftSwipeAction.icon}</span>
            )}
            {leftSwipeAction.label}
          </span>
        </div>
      )}

      {/* Right-swipe action (appears on the left) */}
      {rightSwipeAction && showRight && (
        <div
          className={styles.actionLeft}
          style={{ background: rightSwipeAction.color, opacity: progress }}
        >
          <span className={styles.actionLabel} style={{ opacity: labelOpacity }}>
            {rightSwipeAction.icon && (
              <span className={styles.actionIcon}>{rightSwipeAction.icon}</span>
            )}
            {rightSwipeAction.label}
          </span>
        </div>
      )}

      {/* Sliding content layer */}
      <div
        ref={innerRef}
        className={styles.content}
        style={{
          transform: `translateX(${offset}px)`,
          transition: settling ? "transform 0.25s ease-out" : "none",
        }}
      >
        {children}
      </div>
    </div>
  );
}
