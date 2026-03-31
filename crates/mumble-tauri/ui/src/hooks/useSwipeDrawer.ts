/**
 * useSwipeDrawer - touch-swipe gestures to open/close a left-side drawer.
 *
 * The drawer element follows the finger in real time during the gesture and
 * snaps open or closed on release (with the existing CSS transition).
 *
 * - Swipe right from the left edge opens the drawer.
 * - Swipe left while open closes the drawer.
 * - Mostly-vertical swipes are ignored so normal scrolling still works.
 *
 * @param isOpen      Current open/closed state of the drawer.
 * @param onOpen      Called when the gesture resolves to "open".
 * @param onClose     Called when the gesture resolves to "close".
 * @param options     Thresholds and element refs.
 */

import { useEffect, useRef, type RefObject } from "react";
import { isMobile } from "../utils/platform";

interface SwipeDrawerOptions {
  /** Width of the left-edge zone (px) where a right-swipe triggers open. Default: 30. */
  edgeWidth?: number;
  /** Element ref to listen for touch events on.  Falls back to `document`. */
  containerRef?: RefObject<HTMLElement | null>;
  /** Ref to the drawer element whose transform is manipulated during drag. */
  drawerRef?: RefObject<HTMLElement | null>;
}

/** Percentage of the drawer width that must be revealed to snap open. */
const SNAP_THRESHOLD = 0.35;

export function useSwipeDrawer(
  isOpen: boolean,
  onOpen: () => void,
  onClose: () => void,
  options: SwipeDrawerOptions = {},
): void {
  const { edgeWidth = 30, containerRef, drawerRef } = options;

  // Keep mutable refs so the effect closure always sees the latest values.
  const onOpenRef = useRef(onOpen);
  const onCloseRef = useRef(onClose);
  onOpenRef.current = onOpen;
  onCloseRef.current = onClose;

  const isOpenRef = useRef(isOpen);
  isOpenRef.current = isOpen;

  useEffect(() => {
    if (!isMobile) return;

    const target = containerRef?.current ?? document;

    let startX = 0;
    let startY = 0;
    /** null = not yet decided, true = horizontal drag, false = vertical scroll */
    let gestureAxis: boolean | null = null;
    let dragging = false;

    /** Resolve the drawer width in px (needed to convert dx to %). */
    const getDrawerWidth = (): number =>
      drawerRef?.current?.offsetWidth ?? 300;

    /** Apply a raw translateX (px) to the drawer, bypassing CSS transition. */
    const setDrawerTranslate = (px: number) => {
      const el = drawerRef?.current;
      if (!el) return;
      el.style.transition = "none";
      el.style.transform = `translateX(${px}px)`;
    };

    /** Let CSS transition take over again and clear inline styles. */
    const releaseDrawer = () => {
      const el = drawerRef?.current;
      if (!el) return;
      el.style.transition = "";
      el.style.transform = "";
    };

    // -- Touch handlers ------------------------------------------------

    const onTouchStart = (e: Event) => {
      const touch = (e as TouchEvent).touches[0];
      if (!touch) return;
      startX = touch.clientX;
      startY = touch.clientY;
      gestureAxis = null;
      dragging = false;
    };

    const onTouchMove = (e: Event) => {
      const touch = (e as TouchEvent).touches[0];
      if (!touch) return;

      const dx = touch.clientX - startX;
      const dy = touch.clientY - startY;

      // Lock direction after a small movement.
      if (gestureAxis === null) {
        const absDx = Math.abs(dx);
        const absDy = Math.abs(dy);
        if (absDx < 8 && absDy < 8) return; // too small to decide
        gestureAxis = absDx >= absDy; // true = horizontal
        if (!gestureAxis) return; // vertical => bail out entirely

        // Only start tracking an opening drag from the left edge,
        // or a closing drag when the drawer is already open.
        const edgeSwipe = startX <= edgeWidth && dx > 0;
        const closeSwipe = isOpenRef.current && dx < 0;
        if (!edgeSwipe && !closeSwipe) {
          gestureAxis = false; // not a valid drawer gesture
          return;
        }
        dragging = true;
      }

      if (!dragging) return;

      const drawerWidth = getDrawerWidth();

      if (isOpenRef.current) {
        // Closing: drawer starts at 0, drag pushes it left.
        const clamped = Math.max(-drawerWidth, Math.min(0, dx));
        setDrawerTranslate(clamped);
      } else {
        // Opening: drawer starts at -drawerWidth, drag pulls it right.
        const clamped = Math.max(-drawerWidth, Math.min(0, -drawerWidth + dx));
        setDrawerTranslate(clamped);
      }
    };

    const onTouchEnd = (e: Event) => {
      if (!dragging) return;
      dragging = false;

      const touch = (e as TouchEvent).changedTouches[0];
      if (!touch) {
        releaseDrawer();
        return;
      }

      const dx = touch.clientX - startX;
      const drawerWidth = getDrawerWidth();

      // Remove inline transform so CSS transition kicks in for the snap.
      releaseDrawer();

      if (isOpenRef.current) {
        // Was open - close if dragged past threshold.
        if (-dx > drawerWidth * SNAP_THRESHOLD) {
          onCloseRef.current();
        }
        // Otherwise it stays open (CSS class still applied).
      } else if (dx > drawerWidth * SNAP_THRESHOLD) {
        // Was closed - open if dragged past threshold.
        onOpenRef.current();
      }
      // Otherwise it stays closed (no CSS class).
    };

    const onTouchCancel = () => {
      if (dragging) {
        dragging = false;
        releaseDrawer();
      }
    };

    target.addEventListener("touchstart", onTouchStart, { passive: true });
    target.addEventListener("touchmove", onTouchMove, { passive: true });
    target.addEventListener("touchend", onTouchEnd, { passive: true });
    target.addEventListener("touchcancel", onTouchCancel, { passive: true });

    return () => {
      target.removeEventListener("touchstart", onTouchStart);
      target.removeEventListener("touchmove", onTouchMove);
      target.removeEventListener("touchend", onTouchEnd);
      target.removeEventListener("touchcancel", onTouchCancel);
    };
  }, [containerRef, drawerRef, edgeWidth]);
}
