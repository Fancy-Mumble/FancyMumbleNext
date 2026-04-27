/**
 * PopoutPage - dedicated route rendered inside a frameless,
 * always-on-top webview window spawned by `open_image_popout`.
 *
 * Lifecycle:
 *  1. Read this window's Tauri label (`popout-<id>`) to recover the id.
 *  2. Invoke `take_popout_image` to retrieve and consume the payload
 *     (image src + sender metadata).
 *  3. Render the image fullscreen with a transparent drag handle and a
 *     frosted-glass info bar at the bottom showing the sender, avatar,
 *     optional caption and timestamp.
 *  4. Right-click anywhere opens a small floating menu with:
 *     - "Fit to Image": shrink the window to the currently rendered
 *       image rect, removing letterbox bars without rescaling.
 *     - "Lock Aspect Ratio": persistent toggle that snaps the window
 *       back to the image's aspect ratio on every resize.
 *     - "Close": close the popout window.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import styles from "./PopoutPage.module.css";

interface MenuPos {
  x: number;
  y: number;
}

interface PopoutImagePayload {
  src: string;
  sender_name?: string | null;
  sender_avatar?: string | null;
  caption?: string | null;
  timestamp_ms?: number | null;
}

function popoutIdFromLabel(): string | null {
  try {
    const label = getCurrentWindow().label;
    if (label.startsWith("popout-")) return label.slice("popout-".length);
  } catch {
    // ignore - not running inside a Tauri window (dev mode)
  }
  return new URLSearchParams(window.location.search).get("popout");
}

function formatTimestamp(ms: number | null | undefined): string | null {
  if (!ms || !Number.isFinite(ms)) return null;
  try {
    const date = new Date(ms);
    return date.toLocaleString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return null;
  }
}

function initialFor(name: string | null | undefined): string {
  if (!name) return "?";
  const trimmed = name.trim();
  return trimmed.length > 0 ? trimmed.charAt(0).toUpperCase() : "?";
}

export default function PopoutPage() {
  const [payload, setPayload] = useState<PopoutImagePayload | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuPos | null>(null);
  const [aspectLocked, setAspectLocked] = useState<boolean>(() => {
    try {
      return localStorage.getItem("popout.aspectLocked") === "1";
    } catch {
      return false;
    }
  });
  const [imageLoaded, setImageLoaded] = useState(false);
  // React 19 StrictMode double-invokes effects in dev; the registry
  // entry is single-use, so guard against the second invocation.
  const fetchedRef = useRef(false);
  const imageRef = useRef<HTMLImageElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  // --- Scroll-to-dim -----------------------------------------------
  // The current window opacity stored in a ref so the wheel handler
  // always reads the latest value without needing to re-register.
  // The opacity persists until the user scrolls again - it does NOT
  // auto-reset, so the user can position the dimmed window over other
  // content and leave it there.
  const OPACITY_MIN = 0.15;
  const OPACITY_MAX = 1.0;
  const OPACITY_STEP = 0.05;

  const opacityRef = useRef(OPACITY_MAX);

  const applyOpacity = useCallback((value: number) => {
    const clamped = Math.min(OPACITY_MAX, Math.max(OPACITY_MIN, value));
    opacityRef.current = clamped;
    document.documentElement.style.opacity = String(clamped);
  }, []);

  // Make the host page transparent so the OS-level transparent window
  // (configured via `.transparent(true)` in the Rust window builder)
  // actually shows the desktop behind us.  We override the global body
  // background only while the popout is mounted, then restore it.
  useEffect(() => {
    const html = document.documentElement;
    const body = document.body;
    const prevHtmlBg = html.style.background;
    const prevBodyBg = body.style.background;
    html.style.background = "transparent";
    body.style.background = "transparent";
    return () => {
      html.style.background = prevHtmlBg;
      body.style.background = prevBodyBg;
    };
  }, []);

  useEffect(() => {
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const delta = e.deltaY > 0 ? -OPACITY_STEP : OPACITY_STEP;
      applyOpacity(opacityRef.current + delta);
    };

    window.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      window.removeEventListener("wheel", onWheel);
    };
  }, [applyOpacity]);
  // -----------------------------------------------------------------

  useEffect(() => {
    if (fetchedRef.current) return;
    fetchedRef.current = true;

    const id = popoutIdFromLabel();
    if (!id) {
      setError("Missing popout id");
      return;
    }
    invoke<PopoutImagePayload | null>("take_popout_image", { id })
      .then((result) => {
        if (result) setPayload(result);
        else setError("Image source unavailable");
      })
      .catch((e) => setError(String(e)));
  }, []);

  const handleClose = () => {
    getCurrentWindow().close().catch((e) => console.error("close failed", e));
  };

  const setWindowSize = useCallback((width: number, height: number) => {
    const w = Math.max(1, Math.round(width));
    const h = Math.max(1, Math.round(height));
    getCurrentWindow()
      .setSize(new LogicalSize(w, h))
      .catch((e) => console.error("setSize failed", e));
  }, []);

  const handleFitToImage = () => {
    closeMenu();
    const img = imageRef.current;
    if (!img) return;

    // Use the rendered (CSS-pixel) size of the <img>, not naturalWidth/Height.
    // The image is letterboxed inside the window via `object-fit: contain`,
    // so its rect is exactly the visible image area - sizing the window to
    // that rect collapses the grey bars without changing how big the image
    // looks on screen.
    const rect = img.getBoundingClientRect();
    if (!rect.width || !rect.height) return;

    setWindowSize(rect.width, rect.height);
  };

  const toggleAspectLock = () => {
    closeMenu();
    setAspectLocked((prev) => {
      const next = !prev;
      try {
        localStorage.setItem("popout.aspectLocked", next ? "1" : "0");
      } catch {
        // ignore - storage may be unavailable
      }
      return next;
    });
  };

  // Native aspect-ratio constraint.  The Rust `set_window_aspect_ratio`
  // command hooks the OS resize loop (WM_SIZING on Windows) so the
  // snapping happens BEFORE the window is repainted - no flicker, no
  // feedback loop with a JS resize listener.
  //
  // On platforms without a native implementation the command resolves
  // to `false`; we currently do not provide a JS fallback (the previous
  // attempt caused the flickering the user reported).
  useEffect(() => {
    if (!imageLoaded) return;
    const img = imageRef.current;
    if (!img) return;

    const ratio = aspectLocked
      ? img.naturalWidth / img.naturalHeight
      : null;
    if (ratio !== null && (!Number.isFinite(ratio) || ratio <= 0)) return;

    invoke<boolean>("set_window_aspect_ratio", { ratio })
      .catch((e) => console.error("set_window_aspect_ratio failed", e));

    return () => {
      // Clear the constraint on unmount / when the lock toggles off,
      // so the user can freely resize again.
      invoke<boolean>("set_window_aspect_ratio", { ratio: null })
        .catch((e) => console.error("set_window_aspect_ratio clear failed", e));
    };
  }, [aspectLocked, imageLoaded]);

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY });
  };

  const closeMenu = () => setMenu(null);

  // Keep the floating menu inside the window: after it renders we
  // measure its bounding box and shift it left/up if it would overflow.
  useLayoutEffect(() => {
    if (!menu || !menuRef.current) return;
    const rect = menuRef.current.getBoundingClientRect();
    const margin = 4;
    const maxX = window.innerWidth - rect.width - margin;
    const maxY = window.innerHeight - rect.height - margin;
    const x = Math.max(margin, Math.min(menu.x, maxX));
    const y = Math.max(margin, Math.min(menu.y, maxY));
    if (x !== menu.x || y !== menu.y) {
      setMenu({ x, y });
    }
  }, [menu]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (menu) closeMenu();
        else handleClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [menu]);

  // Close this popout window when the server connection is lost.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<string | null>("server-disconnected", () => {
      handleClose();
    }).then((unlistenFn) => { unlisten = unlistenFn; }).catch(() => {});
    return () => { unlisten?.(); };
  }, []);

  const timestamp = formatTimestamp(payload?.timestamp_ms);
  const senderName = payload?.sender_name ?? null;
  const caption = payload?.caption ?? null;
  const showInfoBar = !!(senderName || caption || timestamp);

  return (
    <div
      className={styles.popout}
      onContextMenu={handleContextMenu}
      role="presentation"
    >
      <div className={styles.dragHandle} data-tauri-drag-region />
      {error && <div className={styles.error}>{error}</div>}
      {payload && (
        <img
          ref={imageRef}
          src={payload.src}
          alt=""
          className={styles.image}
          draggable={false}
          data-tauri-drag-region
          onContextMenu={handleContextMenu}
          onLoad={() => setImageLoaded(true)}
        />
      )}
      {showInfoBar && (
        <div
          className={styles.infoBar}
          data-tauri-drag-region
          onContextMenu={handleContextMenu}
        >
          {payload?.sender_avatar ? (
            <img
              className={styles.avatar}
              src={payload.sender_avatar}
              alt=""
              draggable={false}
            />
          ) : (
            <div className={styles.avatarFallback} aria-hidden="true">
              {initialFor(senderName)}
            </div>
          )}
          <div className={styles.infoText}>
            <div className={styles.infoTopRow}>
              {senderName && <span className={styles.senderName}>{senderName}</span>}
              {timestamp && <span className={styles.timestamp}>{timestamp}</span>}
            </div>
            {caption && <div className={styles.caption}>{caption}</div>}
          </div>
        </div>
      )}
      {menu && (
        <>
          <div
            className={styles.menuOverlay}
            onClick={closeMenu}
            onContextMenu={(e) => { e.preventDefault(); closeMenu(); }}
            role="presentation"
          />
          <div
            ref={menuRef}
            className={styles.menu}
            style={{ top: menu.y, left: menu.x }}
          >
            <button
              type="button"
              className={styles.menuItem}
              onClick={handleFitToImage}
            >
              Fit to Image
            </button>
            <button
              type="button"
              className={styles.menuItem}
              onClick={toggleAspectLock}
              role="menuitemcheckbox"
              aria-checked={aspectLocked}
            >
              {aspectLocked ? "✓ " : ""}Lock Aspect Ratio
            </button>
            <button
              type="button"
              className={styles.menuItem}
              onClick={() => { closeMenu(); handleClose(); }}
            >
              Close
            </button>
          </div>
        </>
      )}
    </div>
  );
}
