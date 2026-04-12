export type ThemeId = "dark" | "light" | "apprentice" | "storm" | "tokyo-night" | "rose" | "neon" | "hearth" | "macchiato";

export interface ThemeOption {
  readonly id: ThemeId;
  readonly label: string;
  readonly swatches: readonly string[];
}

export const THEMES: readonly ThemeOption[] = [
  { id: "dark", label: "Dark", swatches: ["#0e0e16", "#1a1a2e", "#2aabee", "#7c3aed"] },
  { id: "light", label: "Light", swatches: ["#f5f5f9", "#eaeaf0", "#1a8cd8", "#6d28d9"] },
  { id: "apprentice", label: "Apprentice", swatches: ["#1c1c1c", "#303030", "#5F87AF", "#AF5F5F"] },
  { id: "storm", label: "Storm", swatches: ["#0F0F14", "#343B58", "#34548A", "#5A4A78"] },
  { id: "tokyo-night", label: "Tokyo Night", swatches: ["#1A1B26", "#414868", "#7AA2F7", "#BB9AF7"] },
  { id: "rose", label: "Rose", swatches: ["#1a0f14", "#30202a", "#f472b6", "#e879f9"] },
  { id: "neon", label: "Neon", swatches: ["#292c3d", "#444864", "#fb6fa9", "#7586f5"] },
  { id: "hearth", label: "Hearth", swatches: ["#20111B", "#382830", "#EAA549", "#426A79"] },
  { id: "macchiato", label: "Macchiato", swatches: ["#1E2030", "#363A4F", "#8AADF4", "#F5BDE6"] },
];

export const DEFAULT_THEME: ThemeId = "dark";

export function applyTheme(id: ThemeId): void {
  document.documentElement.setAttribute("data-theme", id);
}

export function getCurrentTheme(): ThemeId {
  return (document.documentElement.getAttribute("data-theme") as ThemeId) ?? DEFAULT_THEME;
}
