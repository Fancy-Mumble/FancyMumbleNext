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
