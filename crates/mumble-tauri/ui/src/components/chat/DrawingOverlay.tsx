/**
 * DrawingOverlay - transparent canvas for collaborative screen-share drawing.
 *
 * When drawing mode is active the canvas captures pointer events and sends
 * strokes to the Mumble server via the `send_draw_stroke` Tauri command.
 * The server relays the stroke to every other Fancy client in the channel,
 * and incoming `draw-stroke` events are painted onto the canvas in real time.
 *
 * When drawing mode is off the canvas has pointer-events:none so it is
 * completely transparent to interactions with the video underneath.
 *
 * Persistence: stroke state lives in a module-level store keyed by channel
 * so it survives unmount/remount (switching channels, switching to/from
 * focused stream view, etc.). A single global `draw-stroke` listener is
 * installed on first use and keeps the store up to date even when no
 * overlay is mounted. Local strokes are stored alongside remote strokes so
 * any redraw (resize, incoming remote stroke) replays them as well.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "../../store";
import styles from "./DrawingOverlay.module.css";
import { TrashIcon } from "../../icons";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface DrawStrokeEvent {
  senderSession: number;
  channelId: number;
  strokeId: string;
  color: number;
  width: number;
  widthFrac?: number | null;
  points: number[];
  isEnd: boolean;
  isClear: boolean;
  /** When true together with isClear, wipes every sender's strokes. */
  clearAll?: boolean;
}

interface RemoteStroke {
  strokeId: string;
  color: number;
  /** Legacy absolute pixel width - used as fallback if widthFrac is null. */
  width: number;
  /** Width as fraction of source content height. Preferred over `width`. */
  widthFrac: number | null;
  /** Normalised [x,y,x,y,...] in source-content space, accumulated across packets. */
  points: number[];
}

// ---------------------------------------------------------------------------
// Module-level stroke store - persists across mount/unmount of the overlay.
// ---------------------------------------------------------------------------

type StrokeMap = Map<string, RemoteStroke>;
type ChangeListener = () => void;

/**
 * Stable per-tab/per-webview instance ID. Used to disambiguate logs
 * coming from the main chat tab, the focused stream view, the
 * desktop drawing-overlay window, etc. (each runs in its own JS
 * realm, so this constant is unique per webview).
 */
const INSTANCE_ID = (() => {
  const rand = Math.random().toString(36).slice(2, 8);
  // Try to derive a hint from the Tauri window label or the URL.
  let hint = "";
  try {
    const params = new URLSearchParams(globalThis.location?.search ?? "");
    if (params.has("draw-overlay")) hint = "draw-overlay";
    else if (params.has("focused-stream")) hint = "focus";
    else hint = "main";
  } catch {
    hint = "?";
  }
  return `${hint}-${rand}`;
})();

function logDraw(event: string, data: Record<string, unknown>): void {
  // eslint-disable-next-line no-console
  console.info(`[draw ${INSTANCE_ID}] ${event}`, data);
}

const channelStrokes = new Map<number, StrokeMap>();
const changeListeners = new Map<number, Set<ChangeListener>>();
let globalListenerInstalled = false;
/** Sessions that belong to this client - server echoes from these are ignored. */
const ownSessions = new Set<number>();

function getChannelStrokes(channelId: number): StrokeMap {
  let map = channelStrokes.get(channelId);
  if (!map) {
    map = new Map();
    channelStrokes.set(channelId, map);
  }
  return map;
}

function notifyChange(channelId: number): void {
  const set = changeListeners.get(channelId);
  if (!set) return;
  for (const fn of set) fn();
}

function subscribeToChannel(channelId: number, fn: ChangeListener): () => void {
  let set = changeListeners.get(channelId);
  if (!set) {
    set = new Set();
    changeListeners.set(channelId, set);
  }
  set.add(fn);
  return () => { set!.delete(fn); };
}

function applyStrokeEvent(payload: DrawStrokeEvent): void {
  // Server echoes our own strokes back; ignore them so we don't duplicate points.
  if (ownSessions.has(payload.senderSession)) {
    logDraw("rx-self-skip", {
      sender: payload.senderSession,
      strokeId: payload.strokeId,
      coords: payload.points.length,
      isEnd: payload.isEnd,
      isClear: payload.isClear,
      clearAll: payload.clearAll ?? false,
    });
    return;
  }
  const strokes = getChannelStrokes(payload.channelId);
  if (payload.isClear) {
    if (payload.clearAll) {
      const removed = strokes.size;
      strokes.clear();
      logDraw("rx-clear-all", { sender: payload.senderSession, removed, channelId: payload.channelId });
      notifyChange(payload.channelId);
      return;
    }
    const prefix = `${payload.senderSession}:`;
    let removed = 0;
    for (const id of [...strokes.keys()]) {
      if (id.startsWith(prefix)) { strokes.delete(id); removed++; }
    }
    logDraw("rx-clear", { sender: payload.senderSession, removed, channelId: payload.channelId });
    notifyChange(payload.channelId);
    return;
  }
  const key = `${payload.senderSession}:${payload.strokeId}`;
  const existing = strokes.get(key);
  const newPoints = existing
    ? [...existing.points, ...payload.points]
    : [...payload.points];
  strokes.set(key, {
    strokeId: key,
    color: payload.color,
    width: payload.width,
    widthFrac: payload.widthFrac ?? null,
    points: newPoints,
  });
  logDraw("rx-stroke", {
    sender: payload.senderSession,
    channelId: payload.channelId,
    strokeId: payload.strokeId,
    isFirst: !existing,
    isEnd: payload.isEnd,
    coordsInPacket: payload.points.length,
    coordsTotal: newPoints.length,
    width: payload.width,
    widthFrac: payload.widthFrac,
  });
  notifyChange(payload.channelId);
}

function ensureGlobalListener(): void {
  if (globalListenerInstalled) return;
  globalListenerInstalled = true;
  logDraw("listener-install", {});
  void listen<DrawStrokeEvent>("draw-stroke", (event) => {
    applyStrokeEvent(event.payload);
  });
}

/**
 * Wipe every stroke authored by `senderSession` from every channel
 * store and notify any live overlays.  Called when a remote user
 * stops their broadcast or disconnects so their annotations vanish
 * from every viewer's canvas.
 */
export function clearStrokesFromSender(senderSession: number): void {
  const prefix = `${senderSession}:`;
  for (const [channelId, strokes] of channelStrokes) {
    let removed = 0;
    for (const id of [...strokes.keys()]) {
      if (id.startsWith(prefix)) { strokes.delete(id); removed++; }
    }
    if (removed > 0) {
      logDraw("local-clear-sender", { sender: senderSession, channelId, removed });
      notifyChange(channelId);
    }
  }
}

/**
 * Wipe every stroke (every sender) for a single channel.  Called
 * locally when the broadcaster stops sharing - their canvas should
 * fully reset, including any annotations viewers had drawn on it.
 */
export function clearAllStrokesInChannel(channelId: number): void {
  const strokes = channelStrokes.get(channelId);
  if (!strokes || strokes.size === 0) return;
  const removed = strokes.size;
  strokes.clear();
  logDraw("local-clear-all", { channelId, removed });
  notifyChange(channelId);
}

// ---------------------------------------------------------------------------
// Colour helpers
// ---------------------------------------------------------------------------

const PALETTE = [
  0xFF_FF_00_00, // red
  0xFF_00_CC_00, // green
  0xFF_00_88_FF, // blue
  0xFF_FF_BB_00, // yellow
  0xFF_FF_00_FF, // magenta
  0xFF_FF_FF_FF, // white
  0xFF_00_00_00, // black
];
const DEFAULT_COLOR = PALETTE[0];
const DEFAULT_WIDTH = 4;

/**
 * Maximum send rate for in-flight stroke updates, in milliseconds.
 *
 * The Mumble server applies a leaky-bucket rate limit per user. The
 * default plugin-message bucket allows ~4 messages/second sustained
 * with 15-message burst, so 300 ms (~3.3 msg/s) keeps a single
 * drawing user comfortably below the throttle even on a server with
 * non-default settings.
 */
const FLUSH_INTERVAL_MS = 300;

function argbToCssColor(argb: number): string {
  const a = ((argb >>> 24) & 0xff) / 255;
  const r = (argb >>> 16) & 0xff;
  const g = (argb >>> 8) & 0xff;
  const b = argb & 0xff;
  return `rgba(${r},${g},${b},${a})`;
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

/**
 * Rectangle (in canvas CSS pixels) that maps to the actual shared
 * source content. With `object-fit: contain`, the video is letterboxed
 * inside the canvas; we draw / accept input only in this sub-rect.
 */
export interface ContentRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Compute the content rect for a video element rendered with
 * `object-fit: contain` inside a canvas of size `canvasW x canvasH`.
 * Falls back to the full canvas if intrinsic dimensions are unknown.
 */
export function computeContentRect(
  videoIntrinsicW: number,
  videoIntrinsicH: number,
  canvasW: number,
  canvasH: number,
): ContentRect {
  if (
    canvasW <= 0 || canvasH <= 0
    || videoIntrinsicW <= 0 || videoIntrinsicH <= 0
  ) {
    return { x: 0, y: 0, w: Math.max(canvasW, 0), h: Math.max(canvasH, 0) };
  }
  const scale = Math.min(canvasW / videoIntrinsicW, canvasH / videoIntrinsicH);
  const w = videoIntrinsicW * scale;
  const h = videoIntrinsicH * scale;
  return { x: (canvasW - w) / 2, y: (canvasH - h) / 2, w, h };
}

function renderStroke(
  ctx: CanvasRenderingContext2D,
  contentRect: ContentRect,
  points: number[],
  color: number,
  pixelWidth: number,
) {
  if (points.length < 4) return;
  ctx.strokeStyle = argbToCssColor(color);
  ctx.lineWidth = pixelWidth;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(
    contentRect.x + points[0] * contentRect.w,
    contentRect.y + points[1] * contentRect.h,
  );
  for (let i = 2; i + 1 < points.length; i += 2) {
    ctx.lineTo(
      contentRect.x + points[i] * contentRect.w,
      contentRect.y + points[i + 1] * contentRect.h,
    );
  }
  ctx.stroke();
}

function strokePixelWidth(stroke: RemoteStroke, contentRect: ContentRect): number {
  if (stroke.widthFrac != null && stroke.widthFrac > 0) {
    return Math.max(1, stroke.widthFrac * contentRect.h);
  }
  return Math.max(1, stroke.width);
}

function redrawAll(
  ctx: CanvasRenderingContext2D,
  contentRect: ContentRect,
  strokes: Map<string, RemoteStroke>,
) {
  ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  for (const stroke of strokes.values()) {
    renderStroke(ctx, contentRect, stroke.points, stroke.color, strokePixelWidth(stroke, contentRect));
  }
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface DrawingOverlayProps {
  /** Mumble channel the screen share belongs to. */
  readonly channelId: number;
  /** Our own session ID (to filter out echoes). */
  readonly ownSession: number;
  /**
   * When `true`, only the canvas is rendered - the floating toolbar
   * is suppressed. Used by the desktop drawing-overlay window where
   * the entire window is click-through (so a toolbar would be
   * unreachable anyway).
   */
  readonly hideToolbar?: boolean;
  /**
   * Reference to the `<video>` element underneath the canvas. Used to
   * compute the content rect (the actual shared-content area within
   * the canvas, accounting for `object-fit: contain` letterboxing).
   * When omitted, the entire canvas is treated as the content rect -
   * appropriate for the desktop overlay window where the window itself
   * is sized to match the source.
   */
  readonly videoRef?: React.RefObject<HTMLVideoElement | null>;
}

export default function DrawingOverlay({ channelId, ownSession, hideToolbar, videoRef }: DrawingOverlayProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  /** Cached content rect in canvas CSS pixels - kept in sync with resize + video metadata. */
  const contentRectRef = useRef<ContentRect>({ x: 0, y: 0, w: 0, h: 0 });

  // The dedicated desktop-overlay window (`hideToolbar = true`) only exists
  // while the broadcaster has drawing enabled, so treat it as permanently
  // active.  In-page overlays consult the global drawing-active set.
  const drawingActiveStore = useAppStore((s) => s.drawingActiveChannels.has(channelId));
  const drawingActive = hideToolbar || drawingActiveStore;
  const [selectedColor, setSelectedColor] = useState(DEFAULT_COLOR);
  const [strokeWidth, setStrokeWidth] = useState(DEFAULT_WIDTH);
  /**
   * True when *this* user is the active screen-sharer for this channel.
   * Drives the "clear everyone's drawings" affordance, which is reserved
   * for the broadcaster (receivers should ignore `clear_all` from
   * non-broadcaster senders).
   */
  const isOwnBroadcaster = useAppStore((s) =>
    !!ownSession && s.broadcastingSessions.has(ownSession),
  );

  // Ephemeral local-stroke bookkeeping.
  const currentStrokeId = useRef<string>("");
  const isPointerDown = useRef(false);
  /** Number of point coordinates (x and y combined) already transmitted for the current stroke. */
  const sentCoordCount = useRef(0);
  /**
   * Timer that periodically flushes pending coords to the server.
   *
   * Mumble servers rate-limit incoming messages (default ~5 msg/s
   * sustained, ~10 burst). When we exceeded that with one packet per
   * 16 coords, the server silently dropped the excess and the
   * receiver saw broken / partial strokes. We now batch coords for
   * up to `FLUSH_INTERVAL_MS` and emit a single packet, so a stroke
   * of any length stays under the server's throttle.
   */
  const flushTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Keep selected color/width accessible in event callbacks without stale closure.
  const colorRef = useRef(selectedColor);
  const widthRef = useRef(strokeWidth);
  useEffect(() => { colorRef.current = selectedColor; }, [selectedColor]);
  useEffect(() => { widthRef.current = strokeWidth; }, [strokeWidth]);

  // Helper: redraw entire canvas from the channel store.
  const redraw = useCallback(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!ctx) return;
    redrawAll(ctx, contentRectRef.current, getChannelStrokes(channelId));
  }, [channelId]);

  // Resize canvas to match its CSS size, then replay all strokes.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const recomputeContentRect = (canvasW: number, canvasH: number) => {
      const video = videoRef?.current;
      if (video && video.videoWidth > 0 && video.videoHeight > 0) {
        contentRectRef.current = computeContentRect(
          video.videoWidth, video.videoHeight, canvasW, canvasH,
        );
      } else {
        contentRectRef.current = { x: 0, y: 0, w: canvasW, h: canvasH };
      }
    };

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;
      const dpr = globalThis.devicePixelRatio ?? 1;
      canvas.width = rect.width * dpr;
      canvas.height = rect.height * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      recomputeContentRect(rect.width, rect.height);
      redrawAll(ctx, contentRectRef.current, getChannelStrokes(channelId));
    };

    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    resize();

    // Also recompute when the video starts playing or its intrinsic
    // dimensions change (the picker may negotiate a different size).
    const video = videoRef?.current;
    const onVideoChange = () => {
      const rect = canvas.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;
      recomputeContentRect(rect.width, rect.height);
      redrawAll(ctx, contentRectRef.current, getChannelStrokes(channelId));
    };
    if (video) {
      video.addEventListener("loadedmetadata", onVideoChange);
      video.addEventListener("resize", onVideoChange);
    }
    return () => {
      observer.disconnect();
      if (video) {
        video.removeEventListener("loadedmetadata", onVideoChange);
        video.removeEventListener("resize", onVideoChange);
      }
    };
  }, [channelId, videoRef]);

  // Subscribe to channel-store changes (driven by the global draw-stroke listener).
  useEffect(() => {
    ensureGlobalListener();
    logDraw("mount", {
      channelId,
      ownSession,
      role: hideToolbar ? "desktop-overlay" : "in-page",
      hasVideoRef: !!videoRef,
    });
    // Only register our session as "own" in the interactive overlay - that
    // window stores local strokes synchronously, so the server echo would
    // be a duplicate. The passive desktop overlay (hideToolbar) lives in
    // a separate webview process whose only source of strokes IS the
    // server echo; if it filtered its own session it would drop every
    // stroke the broadcaster draws.
    if (ownSession && !hideToolbar) ownSessions.add(ownSession);
    const unsubscribe = subscribeToChannel(channelId, redraw);
    // Replay any existing strokes for this channel on (re)mount.
    redraw();
    return () => {
      logDraw("unmount", { channelId, ownSession });
      if (flushTimerRef.current != null) {
        clearInterval(flushTimerRef.current);
        flushTimerRef.current = null;
      }
      unsubscribe();
    };
  }, [channelId, redraw, ownSession, hideToolbar, videoRef]);

  // ---------------------------------------------------------------------------
  // Local pointer drawing
  // ---------------------------------------------------------------------------

  const sendPoints = useCallback(
    (pts: number[], isEnd: boolean) => {
      const cr = contentRectRef.current;
      // Send width as a fraction of source content height so receivers
      // (whose canvas may be a different size) can render at the same
      // visual proportion.
      const widthFrac = cr.h > 0 ? widthRef.current / cr.h : null;
      logDraw("tx-stroke", {
        channelId,
        ownSession,
        strokeId: currentStrokeId.current,
        coordsInPacket: pts.length,
        sentTotal: sentCoordCount.current + pts.length,
        isEnd,
        widthFrac,
        contentRect: { w: cr.w, h: cr.h },
      });
      void invoke("send_draw_stroke", {
        args: {
          channelId,
          strokeId: currentStrokeId.current,
          color: colorRef.current,
          width: widthRef.current,
          widthFrac,
          points: pts,
          isEnd: isEnd,
          isClear: false,
        },
      }).catch((err) => {
        logDraw("tx-error", { error: String(err), strokeId: currentStrokeId.current });
      });
    },
    [channelId, ownSession],
  );

  /** Flush any unsent tail of the current local stroke to peers. */
  const flushPending = useCallback(
    (isEnd: boolean) => {
      const key = `${ownSession}:${currentStrokeId.current}`;
      const stroke = getChannelStrokes(channelId).get(key);
      if (!stroke) return;
      const total = stroke.points.length;
      const start = sentCoordCount.current;
      // First packet must include the starting point; subsequent packets send
      // only newly added coordinates (receiver appends them and draws from the
      // last stored point to the new ones).
      if (total <= start && !isEnd) return;
      const tail = stroke.points.slice(start);
      if (tail.length === 0 && !isEnd) return;
      sendPoints(tail, isEnd);
      sentCoordCount.current = total;
    },
    [channelId, ownSession, sendPoints],
  );

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLCanvasElement>) => {
      if (!drawingActive) return;
      const cr = contentRectRef.current;
      if (cr.w <= 0 || cr.h <= 0) return;
      const rect = e.currentTarget.getBoundingClientRect();
      const px = e.clientX - rect.left;
      const py = e.clientY - rect.top;
      // Reject pointer-down on the letterbox bars - those points have
      // no meaningful position in the shared content's coordinate space.
      if (px < cr.x || px > cr.x + cr.w || py < cr.y || py > cr.y + cr.h) return;
      e.currentTarget.setPointerCapture(e.pointerId);
      isPointerDown.current = true;
      currentStrokeId.current = crypto.randomUUID();
      sentCoordCount.current = 0;
      const x = (px - cr.x) / cr.w;
      const y = (py - cr.y) / cr.h;
      logDraw("pointer-down", {
        channelId,
        ownSession,
        strokeId: currentStrokeId.current,
        normX: x.toFixed(4),
        normY: y.toFixed(4),
        contentRect: { w: cr.w, h: cr.h },
      });

      if (flushTimerRef.current != null) clearInterval(flushTimerRef.current);
      flushTimerRef.current = setInterval(() => {
        flushPending(false);
      }, FLUSH_INTERVAL_MS);

      // Store local stroke in the channel map so it survives any redraw.
      // widthFrac is computed against our own content rect height.
      const widthFrac = widthRef.current / cr.h;
      const key = `${ownSession}:${currentStrokeId.current}`;
      getChannelStrokes(channelId).set(key, {
        strokeId: key,
        color: colorRef.current,
        width: widthRef.current,
        widthFrac,
        points: [x, y],
      });

      const canvas = canvasRef.current;
      const ctx = canvas?.getContext("2d");
      if (canvas && ctx) {
        ctx.strokeStyle = argbToCssColor(colorRef.current);
        ctx.lineWidth = widthRef.current;
        ctx.lineCap = "round";
        ctx.lineJoin = "round";
        ctx.beginPath();
        ctx.moveTo(cr.x + x * cr.w, cr.y + y * cr.h);
      }
    },
    [drawingActive, channelId, ownSession, flushPending],
  );

  const onPointerMove = useCallback(
    (e: React.PointerEvent<HTMLCanvasElement>) => {
      if (!drawingActive || !isPointerDown.current) return;
      const cr = contentRectRef.current;
      if (cr.w <= 0 || cr.h <= 0) return;
      const rect = e.currentTarget.getBoundingClientRect();
      const px = e.clientX - rect.left;
      const py = e.clientY - rect.top;
      // Clamp into content rect so a brief drift onto the letterbox
      // bars still produces a valid normalised coordinate.
      const clampedPx = Math.min(Math.max(px, cr.x), cr.x + cr.w);
      const clampedPy = Math.min(Math.max(py, cr.y), cr.y + cr.h);
      const x = (clampedPx - cr.x) / cr.w;
      const y = (clampedPy - cr.y) / cr.h;

      const key = `${ownSession}:${currentStrokeId.current}`;
      const stroke = getChannelStrokes(channelId).get(key);
      if (stroke) stroke.points.push(x, y);

      const canvas = canvasRef.current;
      const ctx = canvas?.getContext("2d");
      if (ctx) {
        ctx.lineTo(cr.x + x * cr.w, cr.y + y * cr.h);
        ctx.stroke();
      }
      // Flushing is driven by `flushTimerRef` (set up in onPointerDown);
      // the timer keeps packet rate well below the murmur server limit.
    },
    [drawingActive, channelId, ownSession],
  );

  const onPointerUp = useCallback(
    (e: React.PointerEvent<HTMLCanvasElement>) => {
      if (!drawingActive || !isPointerDown.current) return;
      isPointerDown.current = false;
      if (flushTimerRef.current != null) {
        clearInterval(flushTimerRef.current);
        flushTimerRef.current = null;
      }
      const cr = contentRectRef.current;
      if (cr.w <= 0 || cr.h <= 0) {
        logDraw("pointer-up-no-rect", { strokeId: currentStrokeId.current });
        flushPending(true);
        return;
      }
      const rect = e.currentTarget.getBoundingClientRect();
      const px = e.clientX - rect.left;
      const py = e.clientY - rect.top;
      const clampedPx = Math.min(Math.max(px, cr.x), cr.x + cr.w);
      const clampedPy = Math.min(Math.max(py, cr.y), cr.y + cr.h);
      const x = (clampedPx - cr.x) / cr.w;
      const y = (clampedPy - cr.y) / cr.h;

      const key = `${ownSession}:${currentStrokeId.current}`;
      const stroke = getChannelStrokes(channelId).get(key);
      if (stroke) stroke.points.push(x, y);

      logDraw("pointer-up", {
        channelId,
        ownSession,
        strokeId: currentStrokeId.current,
        coordsTotal: stroke?.points.length ?? 0,
        sentSoFar: sentCoordCount.current,
      });
      flushPending(true);
    },
    [drawingActive, channelId, ownSession, flushPending],
  );

  const handleClear = useCallback(() => {
    const strokes = getChannelStrokes(channelId);
    if (isOwnBroadcaster) {
      // Broadcaster wipes EVERY sender's strokes locally and on every viewer.
      strokes.clear();
      notifyChange(channelId);
      void invoke("send_draw_stroke", {
        args: {
          channelId,
          strokeId: crypto.randomUUID(),
          color: 0,
          width: 0,
          points: [],
          isEnd: false,
          isClear: true,
          clearAll: true,
        },
      });
      return;
    }
    const prefix = `${ownSession}:`;
    for (const id of [...strokes.keys()]) {
      if (id.startsWith(prefix)) strokes.delete(id);
    }
    notifyChange(channelId);
    void invoke("send_draw_stroke", {
      args: {
        channelId,
        strokeId: crypto.randomUUID(),
        color: 0,
        width: 0,
        points: [],
        isEnd: false,
        isClear: true,
        clearAll: false,
      },
    });
  }, [channelId, ownSession, isOwnBroadcaster]);

  return (
    <div className={styles.overlayRoot}>
      <canvas
        ref={canvasRef}
        className={`${styles.canvas} ${drawingActive ? styles.canvasActive : ""}`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      />

      {/* Toolbar - colour/width/clear shown only while drawing is active.
          The on/off toggle itself lives in the StreamControls bar below. */}
      {!hideToolbar && drawingActive && (
      <div className={styles.toolbar}>
        {PALETTE.map((c) => (
          <button
            key={c}
            type="button"
            className={`${styles.colorSwatch} ${c === selectedColor ? styles.colorSwatchSelected : ""}`}
            style={{ background: argbToCssColor(c) }}
            onClick={() => setSelectedColor(c)}
            title={`Color ${c.toString(16)}`}
            aria-label={`Select color ${c.toString(16)}`}
            aria-pressed={c === selectedColor}
          />
        ))}

        <input
          type="range"
          min={2}
          max={16}
          value={strokeWidth}
          onChange={(e) => setStrokeWidth(Number(e.target.value))}
          className={styles.widthSlider}
          aria-label="Stroke width"
          title={`Width: ${strokeWidth}px`}
        />

        <button
          type="button"
          className={styles.toolBtn}
          onClick={handleClear}
          title={isOwnBroadcaster ? "Clear everyone's drawings" : "Clear my drawings"}
          aria-label={isOwnBroadcaster ? "Clear everyone's drawings" : "Clear my drawings"}
        >
          <TrashIcon width={16} height={16} />
        </button>
      </div>
      )}
    </div>
  );
}
