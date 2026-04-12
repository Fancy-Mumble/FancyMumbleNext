/** Bundled font family definitions used by the personalization panel and chat view. */
export const FONT_FAMILIES: readonly { id: string; label: string; css: string }[] = [
  { id: "system", label: "System Default", css: "inherit" },
  { id: "inter", label: "Inter", css: "'Inter', sans-serif" },
  { id: "roboto", label: "Roboto", css: "'Roboto', sans-serif" },
  { id: "space-mono", label: "Space Mono", css: "'Space Mono', monospace" },
  { id: "monospace", label: "Monospace", css: "'Cascadia Mono', 'Fira Code', 'Consolas', monospace" },
  { id: "serif", label: "Serif", css: "'Georgia', 'Times New Roman', serif" },
  { id: "humanist", label: "Humanist", css: "'Segoe UI', 'Helvetica Neue', 'Arial', sans-serif" },
  { id: "rounded", label: "Rounded", css: "'Nunito', 'Quicksand', 'Comfortaa', sans-serif" },
] as const;

const SYSTEM_DEFAULT_STACK =
  "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Inter', Roboto, sans-serif";

const fontCssById = new Map(FONT_FAMILIES.map((f) => [f.id, f.css]));

/** Apply a font family globally by updating the `--font-family` CSS variable on `:root`. */
export function applyFont(id: string): void {
  const css = fontCssById.get(id);
  const value = !css || css === "inherit" ? SYSTEM_DEFAULT_STACK : css;
  document.documentElement.style.setProperty("--font-family", value);
}
