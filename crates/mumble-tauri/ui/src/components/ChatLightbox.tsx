/**
 * ChatLightbox - full-screen media viewer with prev/next navigation
 * across all images/videos in the current chat.
 *
 * Arrows are overlaid on the left/right edges of the media and are
 * very transparent by default, becoming visible on hover.
 * Arrow keys (Left/Right) navigate; Escape closes.
 */

import { useState, useEffect, useCallback, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import styles from "./ChatLightbox.module.css";
import type { TimeFormat } from "../types";
import { formatTimestamp } from "../utils/format";

const SWIPE_THRESHOLD = 50;
const DISMISS_THRESHOLD = 120;

export interface ChatMediaItem {
  /** Data-URL or remote URL.  Empty string when the image is offloaded. */
  src: string;
  kind: "image" | "gif" | "video";
  alt: string;
  senderName: string;
  timestamp: number;
  /**
   * When set, the media belongs to an offloaded message and `src` is
   * empty.  The lightbox must call `onLoadOffloaded` to fetch the
   * actual content before it can be displayed.
   */
  offloadedMessageId?: string;
  /** Index within the offloaded message (a message can have multiple media). */
  offloadedMediaIndex?: number;
}

interface Props {
  readonly items: ChatMediaItem[];
  readonly activeIndex: number;
  readonly onClose: () => void;
  readonly onNavigate: (index: number) => void;
  /**
   * Called when the lightbox lands on an offloaded item.
   * Must resolve with the actual src, or null on failure.
   */
  readonly onLoadOffloaded?: (messageId: string, mediaIndex: number) => Promise<string | null>;
  readonly timeFormat?: TimeFormat;
  readonly convertToLocalTime?: boolean;
  readonly systemUses24h?: boolean;
}

function getItemDisplaySrc(item: ChatMediaItem, resolved: Map<string, string>): string {
  if (item.offloadedMessageId) {
    const key = `${item.offloadedMessageId}:${item.offloadedMediaIndex ?? 0}`;
    return resolved.get(key) ?? item.src;
  }
  return item.src;
}

export default function ChatLightbox({
  items,
  activeIndex,
  onClose,
  onNavigate,
  onLoadOffloaded,
  timeFormat = "auto",
  convertToLocalTime = true,
  systemUses24h,
}: Props): ReactNode {
  const item = items[activeIndex];
  const hasPrev = activeIndex > 0;
  const hasNext = activeIndex < items.length - 1;

  // Resolved src for offloaded items, keyed by "messageId:mediaIndex".
  const [resolvedSrcs, setResolvedSrcs] = useState<Map<string, string>>(new Map());
  const [loadingKey, setLoadingKey] = useState<string | null>(null);

  const offloadKey = item?.offloadedMessageId
    ? `${item.offloadedMessageId}:${item.offloadedMediaIndex ?? 0}`
    : null;
  const isOffloaded = !!offloadKey && !item.src;
  const resolvedSrc = offloadKey ? resolvedSrcs.get(offloadKey) ?? "" : item?.src ?? "";
  const isLoading = isOffloaded && !resolvedSrc && loadingKey === offloadKey;
  const needsLoad = isOffloaded && !resolvedSrc && loadingKey !== offloadKey;

  // Trigger on-demand load when we land on an offloaded item.
  useEffect(() => {
    if (!needsLoad || !item?.offloadedMessageId || !onLoadOffloaded) return;
    const key = offloadKey!;
    setLoadingKey(key);
    onLoadOffloaded(item.offloadedMessageId, item.offloadedMediaIndex ?? 0)
      .then((src) => {
        if (src) setResolvedSrcs((prev) => new Map(prev).set(key, src));
      })
      .finally(() => setLoadingKey((cur) => cur === key ? null : cur));
  }, [needsLoad, offloadKey, item?.offloadedMessageId, item?.offloadedMediaIndex, onLoadOffloaded]);

  const displaySrc = resolvedSrc || item?.src || "";

  const goPrev = useCallback(() => {
    if (activeIndex > 0) onNavigate(activeIndex - 1);
  }, [activeIndex, onNavigate]);

  const goNext = useCallback(() => {
    if (activeIndex < items.length - 1) onNavigate(activeIndex + 1);
  }, [activeIndex, items.length, onNavigate]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      else if (e.key === "ArrowLeft") goPrev();
      else if (e.key === "ArrowRight") goNext();
    };
    globalThis.addEventListener("keydown", handler);
    return () => globalThis.removeEventListener("keydown", handler);
  }, [onClose, goPrev, goNext]);

  // --- Zoom & Pan ---
  const [zoom, setZoom] = useState(1);
  const [panOffset, setPanOffset] = useState({ x: 0, y: 0 });
  const [isZoomTransition, setIsZoomTransition] = useState(false);
  const zoomRef = useRef(1);
  const panOffsetRef = useRef({ x: 0, y: 0 });

  const updateZoom = useCallback((z: number) => {
    zoomRef.current = z;
    setZoom(z);
  }, []);

  const updatePan = useCallback((p: { x: number; y: number }) => {
    panOffsetRef.current = p;
    setPanOffset(p);
  }, []);

  const resetZoom = useCallback((animated = false) => {
    updateZoom(1);
    updatePan({ x: 0, y: 0 });
    if (animated) {
      setIsZoomTransition(true);
      setTimeout(() => setIsZoomTransition(false), 250);
    }
  }, [updateZoom, updatePan]);

  // Reset zoom on navigate
  useEffect(() => { resetZoom(); }, [activeIndex, resetZoom]);

  // --- Touch gesture tracking ---
  const overlayRef = useRef<HTMLDivElement>(null);
  const touchStartX = useRef<number | null>(null);
  const touchStartY = useRef<number | null>(null);
  const lockedAxis = useRef<"h" | "v" | null>(null);
  const gestureMode = useRef<"none" | "swipe" | "pan" | "pinch" | "dismiss">("none");
  const pinchStartDist = useRef(0);
  const pinchStartZoom = useRef(1);
  const panStart = useRef({ x: 0, y: 0 });
  const panBase = useRef({ x: 0, y: 0 });
  const lastTapTime = useRef(0);
  const [swipeOffset, setSwipeOffset] = useState(0);
  const [isSnapping, setIsSnapping] = useState(false);
  const [dismissOffset, setDismissOffset] = useState(0);
  const [isDismissSnapping, setIsDismissSnapping] = useState(false);
  const [isDismissing, setIsDismissing] = useState(false);

  const handleTouchStart = useCallback((e: React.TouchEvent) => {
    if (e.touches.length === 2) {
      gestureMode.current = "pinch";
      lockedAxis.current = null;
      setSwipeOffset(0);
      setIsSnapping(false);
      const dist = Math.hypot(
        e.touches[1].clientX - e.touches[0].clientX,
        e.touches[1].clientY - e.touches[0].clientY,
      );
      pinchStartDist.current = dist;
      pinchStartZoom.current = zoomRef.current;
      return;
    }

    touchStartX.current = e.touches[0].clientX;
    touchStartY.current = e.touches[0].clientY;

    if (zoomRef.current > 1.05) {
      gestureMode.current = "pan";
      panStart.current = { x: e.touches[0].clientX, y: e.touches[0].clientY };
      panBase.current = { ...panOffsetRef.current };
    } else {
      gestureMode.current = "swipe";
      lockedAxis.current = null;
      setIsSnapping(false);
      setSwipeOffset(0);
      setDismissOffset(0);
      setIsDismissSnapping(false);
    }
  }, []);

  const handleTouchMove = useCallback((e: TouchEvent) => {
    if (gestureMode.current === "pinch" && e.touches.length >= 2) {
      e.preventDefault();
      const dist = Math.hypot(
        e.touches[1].clientX - e.touches[0].clientX,
        e.touches[1].clientY - e.touches[0].clientY,
      );
      const newZoom = (dist / pinchStartDist.current) * pinchStartZoom.current;
      updateZoom(Math.max(0.5, Math.min(5, newZoom)));
      return;
    }

    if (gestureMode.current === "pan") {
      e.preventDefault();
      const dx = e.touches[0].clientX - panStart.current.x;
      const dy = e.touches[0].clientY - panStart.current.y;
      updatePan({ x: panBase.current.x + dx, y: panBase.current.y + dy });
      return;
    }

    if (gestureMode.current === "swipe") {
      if (touchStartX.current === null || touchStartY.current === null) return;
      const dx = e.touches[0].clientX - touchStartX.current;
      const dy = e.touches[0].clientY - touchStartY.current;
      if (!lockedAxis.current && (Math.abs(dx) > 8 || Math.abs(dy) > 8)) {
        lockedAxis.current = Math.abs(dx) >= Math.abs(dy) ? "h" : "v";
        if (lockedAxis.current === "v") {
          gestureMode.current = "dismiss";
        }
      }
      if (lockedAxis.current === "h") {
        let offset = dx;
        if ((dx > 0 && !hasPrev) || (dx < 0 && !hasNext)) offset = dx * 0.3;
        setSwipeOffset(offset);
      }
    }

    if (gestureMode.current === "dismiss") {
      if (touchStartY.current === null) return;
      const dy = e.touches[0].clientY - touchStartY.current;
      setDismissOffset(dy);
    }
  }, [hasPrev, hasNext, updateZoom, updatePan]);

  // Attach touchmove via DOM API with { passive: false } so preventDefault()
  // works on Android WebView (React registers touch listeners as passive).
  useEffect(() => {
    const el = overlayRef.current;
    if (!el) return;
    el.addEventListener("touchmove", handleTouchMove, { passive: false });
    return () => el.removeEventListener("touchmove", handleTouchMove);
  }, [handleTouchMove]);

  const handleTouchEnd = useCallback((e: React.TouchEvent) => {
    const mode = gestureMode.current;

    if (mode === "pinch") {
      // Finger lifted during pinch - if another finger remains, switch to pan
      if (e.touches.length > 0 && zoomRef.current > 1.05) {
        gestureMode.current = "pan";
        panStart.current = { x: e.touches[0].clientX, y: e.touches[0].clientY };
        panBase.current = { ...panOffsetRef.current };
        return;
      }
      gestureMode.current = "none";
      if (zoomRef.current < 1.1) resetZoom(true);
      return;
    }

    // Double-tap detection (only on media elements)
    if (touchStartX.current !== null && touchStartY.current !== null) {
      const dx = e.changedTouches[0].clientX - touchStartX.current;
      const dy = e.changedTouches[0].clientY - touchStartY.current;
      const target = e.target as HTMLElement;
      const isOnMedia = !!target.closest("img, video");

      if (Math.abs(dx) < 10 && Math.abs(dy) < 10 && isOnMedia) {
        const now = Date.now();
        if (now - lastTapTime.current < 300) {
          lastTapTime.current = 0;
          gestureMode.current = "none";
          touchStartX.current = null;
          touchStartY.current = null;
          setIsZoomTransition(true);
          if (zoomRef.current > 1.05) {
            resetZoom(true);
          } else {
            updateZoom(2.5);
            updatePan({ x: 0, y: 0 });
            setTimeout(() => setIsZoomTransition(false), 250);
          }
          return;
        }
        lastTapTime.current = now;
      }
    }

    if (mode === "dismiss") {
      if (touchStartY.current === null) {
        gestureMode.current = "none";
        return;
      }
      const dy = e.changedTouches[0].clientY - touchStartY.current;
      touchStartX.current = null;
      touchStartY.current = null;
      gestureMode.current = "none";

      if (Math.abs(dy) > DISMISS_THRESHOLD) {
        setIsDismissing(true);
        setIsDismissSnapping(true);
        setDismissOffset(dy < 0 ? -globalThis.innerHeight : globalThis.innerHeight);
        setTimeout(onClose, 200);
      } else {
        setIsDismissSnapping(true);
        setDismissOffset(0);
        setTimeout(() => setIsDismissSnapping(false), 200);
      }
      return;
    }

    if (mode === "pan") {
      gestureMode.current = "none";
      touchStartX.current = null;
      touchStartY.current = null;
      return;
    }

    if (mode === "swipe") {
      if (touchStartX.current === null || touchStartY.current === null) {
        gestureMode.current = "none";
        return;
      }
      const dx = e.changedTouches[0].clientX - touchStartX.current;
      touchStartX.current = null;
      touchStartY.current = null;

      if (lockedAxis.current !== "h") {
        lockedAxis.current = null;
        setSwipeOffset(0);
        setDismissOffset(0);
        gestureMode.current = "none";
        return;
      }
      lockedAxis.current = null;

      const canNavigate = dx < 0 ? hasNext : hasPrev;
      if (Math.abs(dx) > SWIPE_THRESHOLD && canNavigate) {
        setIsSnapping(true);
        setSwipeOffset(dx < 0 ? -globalThis.innerWidth : globalThis.innerWidth);
        setTimeout(() => {
          if (dx < 0) goNext();
          else goPrev();
          setSwipeOffset(0);
          setIsSnapping(false);
        }, 200);
      } else {
        setIsSnapping(true);
        setSwipeOffset(0);
        setTimeout(() => setIsSnapping(false), 200);
      }
    }
    gestureMode.current = "none";
  }, [goNext, goPrev, hasPrev, hasNext, resetZoom, updateZoom, updatePan]);

  if (!item) return null;

  const prevItem = hasPrev ? items[activeIndex - 1] : null;
  const nextItem = hasNext ? items[activeIndex + 1] : null;
  const prevSrc = prevItem ? getItemDisplaySrc(prevItem, resolvedSrcs) : "";
  const nextSrc = nextItem ? getItemDisplaySrc(nextItem, resolvedSrcs) : "";

  const trackStyle: React.CSSProperties = {
    transform: `translateX(calc(-100vw + ${swipeOffset}px))`,
    transition: isSnapping ? "transform 0.2s ease-out" : "none",
    willChange: swipeOffset !== 0 || isSnapping ? "transform" : undefined,
  };

  const zoomStyle: React.CSSProperties = zoom !== 1 || isZoomTransition
    ? {
        transform: `translate(${panOffset.x}px, ${panOffset.y}px) scale(${zoom})`,
        transition: isZoomTransition ? "transform 0.25s ease-out" : "none",
        willChange: "transform",
      }
    : {};

  const dismissProgress = Math.min(1, Math.abs(dismissOffset) / 300);
  const overlayOpacity = isDismissing ? 0 : 1 - dismissProgress * 0.6;

  const dismissStyle: React.CSSProperties = dismissOffset !== 0 || isDismissSnapping
    ? {
        transform: `translateY(${dismissOffset}px) scale(${1 - dismissProgress * 0.1})`,
        transition: isDismissSnapping ? "transform 0.2s ease-out" : "none",
        willChange: "transform",
      }
    : {};

  const overlayStyle: React.CSSProperties = {
    background: `rgba(0, 0, 0, ${0.8 * overlayOpacity})`,
    ...(isDismissing ? { transition: "background 0.2s ease-out" } : {}),
  };

  return createPortal(
    <div
      ref={overlayRef}
      className={styles.overlay}
      role="dialog"
      aria-label="Media viewer"
      style={overlayStyle}
      onClick={(e) => {
        const t = e.target as HTMLElement;
        if (!t.closest("img, video, button") && zoomRef.current <= 1.05) onClose();
      }}
      onTouchStart={handleTouchStart}
      onTouchEnd={handleTouchEnd}
    >
      {/* Close */}
      <button type="button" className={styles.close} onClick={onClose}>
        &#x2715;
      </button>

      {/* Carousel: prev | current | next */}
      <div className={styles.carousel} style={dismissStyle}>
        <div className={styles.track} style={trackStyle}>
          <div className={styles.slide}>
            {prevItem && prevSrc && (
              prevItem.kind === "video"
                ? <video className={styles.media} src={prevSrc}><track kind="captions" /></video>
                : <img className={styles.media} src={prevSrc} alt={prevItem.alt} draggable={false} />
            )}
          </div>

          <div className={styles.slide}>
            <div className={styles.zoomContainer} style={zoomStyle}>
              {(isLoading || (isOffloaded && !displaySrc)) ? (
                <div className={styles.loadingPlaceholder}>
                  <div className={styles.spinner} />
                  <span className={styles.loadingLabel}>Loading image&#x2026;</span>
                </div>
              ) : item.kind === "video" ? (
                <video className={styles.media} src={displaySrc} controls autoPlay>
                  <track kind="captions" />
                </video>
              ) : (
                <img className={styles.media} src={displaySrc} alt={item.alt} draggable={false} />
              )}
            </div>
          </div>

          <div className={styles.slide}>
            {nextItem && nextSrc && (
              nextItem.kind === "video"
                ? <video className={styles.media} src={nextSrc}><track kind="captions" /></video>
                : <img className={styles.media} src={nextSrc} alt={nextItem.alt} draggable={false} />
            )}
          </div>
        </div>
      </div>

      {/* Prev / Next arrows */}
      {hasPrev && (
        <button
          type="button"
          className={`${styles.arrow} ${styles.arrowPrev}`}
          onClick={goPrev}
          aria-label="Previous image"
        >
          &#x2039;
        </button>
      )}
      {hasNext && (
        <button
          type="button"
          className={`${styles.arrow} ${styles.arrowNext}`}
          onClick={goNext}
          aria-label="Next image"
        >
          &#x203A;
        </button>
      )}

      {/* Caption */}
      <div className={styles.caption} style={dismissStyle}>
        {items.length > 1 && (
          <span className={styles.counter}>
            Photo {activeIndex + 1} / {items.length}
          </span>
        )}
        <div className={styles.senderRow}>
          <span className={styles.sender}>{item.senderName}</span>
          <time
            className={styles.time}
            dateTime={new Date(item.timestamp).toISOString()}
          >
            {formatTimestamp(item.timestamp, timeFormat, convertToLocalTime, systemUses24h)}
          </time>
        </div>
      </div>
    </div>,
    document.body,
  );
}
