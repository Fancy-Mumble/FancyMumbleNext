/**
 * DrawOverlayPage - dedicated route rendered inside the desktop
 * drawing-overlay window (`draw-overlay`).
 *
 * The window itself is configured by the Rust `open_drawing_overlay`
 * command to be:
 *  - transparent (sits over the real desktop)
 *  - always-on-top
 *  - click-through (`set_ignore_cursor_events(true)`)
 *  - excluded from screen capture (`WDA_EXCLUDEFROMCAPTURE` /
 *    `NSWindowSharingNone`)
 *
 * The page makes its document tree fully transparent and renders the
 * existing `DrawingOverlay` component with the toolbar suppressed.
 * Strokes from peers arrive via the same module-level `draw-stroke`
 * event that `DrawingOverlay` already listens for - Tauri broadcasts
 * `app.emit(...)` to every webview window in the process.
 */

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import DrawingOverlay from "../components/chat/DrawingOverlay";
import styles from "./DrawOverlayPage.module.css";

interface DrawOverlayContext {
  channel_id: number;
  own_session: number;
}

export default function DrawOverlayPage() {
  const [ctx, setCtx] = useState<DrawOverlayContext | null>(null);
  const [error, setError] = useState<string | null>(null);

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
    invoke<DrawOverlayContext | null>("take_drawing_overlay_context")
      .then((result) => {
        if (result) setCtx(result);
        else setError("Drawing overlay context unavailable");
      })
      .catch((e) => setError(String(e)));
  }, []);

  if (error) {
    return <div className={styles.error}>{error}</div>;
  }
  if (!ctx) {
    return <div className={styles.root} />;
  }

  return (
    <div className={styles.root}>
      <DrawingOverlay
        channelId={ctx.channel_id}
        ownSession={ctx.own_session}
        hideToolbar
      />
    </div>
  );
}
