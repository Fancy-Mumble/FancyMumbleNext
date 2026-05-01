import { useEffect } from "react";

/**
 * Globally listens for clicks on `<span class="spoiler">` elements and
 * toggles the `revealed` modifier so the obscured text becomes visible.
 *
 * Mounted once at the app root - works for any HTML rendered through
 * SafeHtml or markdownToHtml without each renderer needing to wire
 * its own handler.
 */
export function useSpoilerReveal(): void {
  useEffect(() => {
    const onClick = (event: MouseEvent) => {
      const target = event.target as HTMLElement | null;
      if (!target) return;
      const spoiler = target.closest<HTMLElement>("span.spoiler");
      if (!spoiler || spoiler.classList.contains("revealed")) return;
      spoiler.classList.add("revealed");
      event.stopPropagation();
    };
    document.addEventListener("click", onClick, true);
    return () => document.removeEventListener("click", onClick, true);
  }, []);
}
