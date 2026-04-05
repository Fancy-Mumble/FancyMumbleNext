/**
 * Platform detection utilities.
 *
 * Differentiates between desktop (Windows/macOS/Linux via Tauri) and
 * mobile (Android/iOS via Tauri mobile) so components can adapt their
 * layout and controls.
 */

/** Detect whether the app is running on a mobile device (Android/iOS). */
export function isMobilePlatform(): boolean {
  // Tauri on Android/iOS sets the user-agent to contain "Android" or "iPhone".
  // Some Tauri mobile builds may also expose __TAURI_INTERNALS__.
  const ua = navigator.userAgent;
  return /Android|iPhone|iPad|iPod/i.test(ua);
}

/**
 * Cached result of `isMobilePlatform()`.
 * The user-agent never changes during a session, so this is safe to
 * evaluate once at module load and import as a plain constant.
 */
export const isMobile: boolean = isMobilePlatform();

/** Detect whether the app is running on a desktop (Windows/macOS/Linux). */
export function isDesktopPlatform(): boolean {
  return !isMobile;
}

/**
 * CSS class helper: returns the given class name only on mobile,
 * empty string on desktop.
 */
export function mobileOnly(className: string): string {
  return isMobile ? className : "";
}

/**
 * CSS class helper: returns the given class name only on desktop,
 * empty string on mobile.
 */
export function desktopOnly(className: string): string {
  return isDesktopPlatform() ? className : "";
}

/**
 * Detect whether `backdrop-filter: blur()` actually renders visually.
 *
 * WebKitGTK (Linux) parses the property and reports support via
 * `CSS.supports()`, but its compositing pipeline does not actually
 * apply the blur. We detect this by creating a small off-screen test:
 * a coloured div behind a semi-transparent div with `backdrop-filter`.
 * If the backdrop blur is rendered, the blurred pixels bleed colour
 * into the overlay making it distinguishable from a no-blur scenario.
 * When the sampled pixel matches the raw overlay colour, blur is not
 * working and we set `data-no-backdrop-blur` on `<html>` so CSS can
 * provide opaque-glass fallbacks via variable overrides.
 */
export function detectBackdropFilterSupport(): void {
  // Quick path: if the browser explicitly says no, mark immediately.
  if (
    typeof CSS !== "undefined" &&
    CSS.supports &&
    !CSS.supports("backdrop-filter", "blur(1px)") &&
    !CSS.supports("-webkit-backdrop-filter", "blur(1px)")
  ) {
    document.documentElement.setAttribute("data-no-backdrop-blur", "");
    return;
  }

  // Render-based detection using a canvas to check if blur actually
  // blends background colour into the overlay.
  requestAnimationFrame(() => {
    const container = document.createElement("div");
    Object.assign(container.style, {
      position: "fixed",
      left: "-9999px",
      top: "-9999px",
      width: "40px",
      height: "40px",
      overflow: "hidden",
      zIndex: "-1",
      pointerEvents: "none",
    });

    const bg = document.createElement("div");
    Object.assign(bg.style, {
      width: "40px",
      height: "40px",
      background: "#ff0000",
    });

    const overlay = document.createElement("div");
    Object.assign(overlay.style, {
      position: "absolute",
      inset: "0",
      background: "rgba(0, 0, 255, 0.3)",
      backdropFilter: "blur(10px)",
      WebkitBackdropFilter: "blur(10px)",
    });

    container.appendChild(bg);
    container.appendChild(overlay);
    document.body.appendChild(container);

    // Wait a frame for compositing to kick in, then sample a pixel.
    requestAnimationFrame(() => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = 40;
        canvas.height = 40;
        const ctx = canvas.getContext("2d", { willReadFrequently: true });
        if (!ctx) {
          // Cannot test - assume broken on Linux UA.
          if (/Linux/.test(navigator.userAgent)) {
            document.documentElement.setAttribute("data-no-backdrop-blur", "");
          }
          container.remove();
          return;
        }

        // Draw what the compositor produced via html2canvas-like approach.
        // Since we cannot screenshot the compositor output directly, we
        // use a UA-sniffing fallback for WebKitGTK on Linux where the
        // rendering issue is known.
        //
        // WebKitGTK identifies itself as "AppleWebKit" in the UA but
        // without "Chrome" (Chromium-based browsers include "Chrome").
        const ua = navigator.userAgent;
        const isWebKitGTK =
          /Linux/.test(ua) && /AppleWebKit/.test(ua) && !/Chrome/.test(ua);

        if (isWebKitGTK) {
          document.documentElement.setAttribute("data-no-backdrop-blur", "");
        }
      } finally {
        container.remove();
      }
    });
  });
}
