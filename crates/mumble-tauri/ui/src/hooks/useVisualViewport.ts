/**
 * useVisualViewport - tracks the visual viewport height so the layout
 * can shrink when the on-screen keyboard is active on mobile.
 *
 * Sets a CSS custom property `--app-height` on `document.documentElement`
 * that equals the actual visible area.  On desktop (or when the Visual
 * Viewport API is unavailable) it falls back to `100%`.
 *
 * Usage: call once near the root of the application (e.g. in `App`).
 */

import { useEffect } from "react";
import { isMobile } from "../utils/platform";

export function useVisualViewport(): void {
  useEffect(() => {
    if (!isMobile) return;

    const vv = globalThis.visualViewport;
    if (!vv) return;

    const update = () => {
      // visualViewport.height is the visible area excluding the
      // on-screen keyboard.  Using `px` ensures the layout shrinks
      // to exactly what is available.
      const h = vv.height;
      document.documentElement.style.setProperty("--app-height", `${h}px`);
    };

    update();
    vv.addEventListener("resize", update);
    vv.addEventListener("scroll", update);

    return () => {
      vv.removeEventListener("resize", update);
      vv.removeEventListener("scroll", update);
      document.documentElement.style.removeProperty("--app-height");
    };
  }, []);
}
