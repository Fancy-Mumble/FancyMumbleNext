import { load } from "@tauri-apps/plugin-store";
import type React from "react";
import type { FancyProfile } from "../../types";

export interface ProfileData {
  profile: FancyProfile;
  bio: string;
  avatarDataUrl: string | null;
}

const PROFILE_STORE = "profile.json";

const PROFILE_DEFAULTS: ProfileData = {
  profile: {},
  bio: "",
  avatarDataUrl: null,
};

export async function loadProfileData(): Promise<ProfileData> {
  const store = await load(PROFILE_STORE, { autoSave: true, defaults: {} });
  const data = await store.get<ProfileData>("data");
  return data ? { ...PROFILE_DEFAULTS, ...data } : { ...PROFILE_DEFAULTS };
}

export async function saveProfileData(data: ProfileData): Promise<void> {
  const store = await load(PROFILE_STORE, { autoSave: true, defaults: {} });
  await store.set("data", data);
}

export const DECORATIONS: { id: string; label: string; preview: string }[] = [
  { id: "none", label: "None", preview: "-" },
  { id: "sparkle", label: "Sparkle", preview: "✨" },
  { id: "fire", label: "Fire", preview: "🔥" },
  { id: "ice", label: "Ice", preview: "❄️" },
  { id: "rainbow", label: "Rainbow", preview: "🌈" },
  { id: "gold", label: "Gold", preview: "👑" },
  { id: "neon", label: "Neon", preview: "💜" },
  { id: "glitch", label: "Glitch", preview: "⚡" },
];

export const NAMEPLATES: { id: string; label: string; bg: string }[] = [
  { id: "none", label: "None", bg: "transparent" },
  { id: "gradient_blue", label: "Ocean", bg: "linear-gradient(135deg,#667eea,#764ba2)" },
  { id: "gradient_purple", label: "Amethyst", bg: "linear-gradient(135deg,#a855f7,#6366f1)" },
  { id: "gradient_sunset", label: "Sunset", bg: "linear-gradient(135deg,#f97316,#ef4444)" },
  { id: "gold", label: "Gold", bg: "linear-gradient(135deg,#fbbf24,#d97706)" },
  { id: "silver", label: "Silver", bg: "linear-gradient(135deg,#d1d5db,#9ca3af)" },
  { id: "rainbow", label: "Rainbow", bg: "linear-gradient(135deg,#ef4444,#f97316,#eab308,#22c55e,#3b82f6,#8b5cf6)" },
  { id: "dark", label: "Dark", bg: "linear-gradient(135deg,#1f2937,#111827)" },
];

export const EFFECTS: {
  id: string;
  label: string;
  preview: string;
  /** CSS animation / filter / overlay description for rendering. */
  css: React.CSSProperties;
  /** Optional keyframes name (defined in the CSS module). */
  animation?: string;
}[] = [
  { id: "none", label: "None", preview: "-", css: {} },
  {
    id: "particles",
    label: "Particles",
    preview: "🫧",
    css: {},
    animation: "effectParticles",
  },
  {
    id: "sparkle",
    label: "Sparkle",
    preview: "⭐",
    css: {},
    animation: "effectSparkle",
  },
  {
    id: "snow",
    label: "Snow",
    preview: "🌨️",
    css: {},
    animation: "effectSnow",
  },
  {
    id: "rain",
    label: "Rain",
    preview: "🌧️",
    css: {},
    animation: "effectRain",
  },
  {
    id: "fireflies",
    label: "Fireflies",
    preview: "🪲",
    css: {},
    animation: "effectFireflies",
  },
  {
    id: "pulse_glow",
    label: "Pulse Glow",
    preview: "💜",
    css: {},
    animation: "effectPulseGlow",
  },
  {
    id: "rainbow_shift",
    label: "Rainbow Shift",
    preview: "🌈",
    css: {},
    animation: "effectRainbowShift",
  },
  {
    id: "vignette",
    label: "Vignette",
    preview: "🔲",
    css: {
      boxShadow: "inset 0 0 40px rgba(0,0,0,0.6)",
    },
  },
];

export const FONTS: { id: string; label: string; css: string }[] = [
  { id: "default", label: "Default", css: "inherit" },
  { id: "serif", label: "Serif", css: "Georgia, serif" },
  { id: "mono", label: "Monospace", css: "'Courier New', monospace" },
  { id: "cursive", label: "Cursive", css: "'Segoe Script', cursive" },
  { id: "fantasy", label: "Fantasy", css: "'Impact', fantasy" },
];

// ─── Card Backgrounds ──────────────────────────────────────────────

export const CARD_BACKGROUNDS: {
  id: string;
  label: string;
  /** CSS background value or shorthand. */
  value: string;
  /** Extra CSS properties (e.g. backdrop-filter). */
  extra?: React.CSSProperties;
}[] = [
  {
    id: "default",
    label: "Default",
    value: "var(--color-glass)",
  },
  {
    id: "dark",
    label: "Dark",
    value: "rgba(10, 10, 15, 0.95)",
  },
  {
    id: "midnight",
    label: "Midnight",
    value: "linear-gradient(145deg, #0f0c29, #302b63, #24243e)",
  },
  {
    id: "ocean",
    label: "Ocean",
    value: "linear-gradient(135deg, #0f2027, #203a43, #2c5364)",
  },
  {
    id: "sunset",
    label: "Sunset",
    value: "linear-gradient(135deg, #2d1b38, #44224a, #5e2e53)",
  },
  {
    id: "forest",
    label: "Forest",
    value: "linear-gradient(135deg, #0b1a0f, #1a3a1f, #0f2a14)",
  },
  {
    id: "ember",
    label: "Ember",
    value: "linear-gradient(135deg, #1a0a00, #3a1508, #2a0f05)",
  },
  {
    id: "glass_light",
    label: "Glass (Light)",
    value: "rgba(255, 255, 255, 0.08)",
    extra: { backdropFilter: "blur(16px) saturate(1.4)" },
  },
  {
    id: "glass_dark",
    label: "Glass (Dark)",
    value: "rgba(0, 0, 0, 0.35)",
    extra: { backdropFilter: "blur(16px) saturate(1.2)" },
  },
  {
    id: "glass_purple",
    label: "Glass (Purple)",
    value: "rgba(99, 102, 241, 0.12)",
    extra: { backdropFilter: "blur(16px) saturate(1.6)" },
  },
  {
    id: "transparent",
    label: "Transparent",
    value: "transparent",
  },
  {
    id: "custom",
    label: "Custom…",
    value: "",
  },
];

// ─── Avatar Borders ────────────────────────────────────────────────

export const AVATAR_BORDERS: {
  id: string;
  label: string;
  /** CSS border shorthand. */
  border: string;
  /** Optional box-shadow for glow effects. */
  shadow?: string;
  /** Optional outline. */
  outline?: string;
}[] = [
  { id: "default", label: "Default", border: "3px solid var(--color-glass)" },
  { id: "none", label: "None", border: "none" },
  { id: "thin_white", label: "Thin White", border: "2px solid rgba(255,255,255,0.7)" },
  { id: "thick_white", label: "Thick White", border: "4px solid rgba(255,255,255,0.9)" },
  {
    id: "gold",
    label: "Gold",
    border: "3px solid #d4a017",
    shadow: "0 0 8px rgba(212,160,23,0.5)",
  },
  {
    id: "silver",
    label: "Silver",
    border: "3px solid #b0b0b0",
    shadow: "0 0 6px rgba(176,176,176,0.4)",
  },
  {
    id: "neon_blue",
    label: "Neon Blue",
    border: "2px solid #00d4ff",
    shadow: "0 0 10px #00d4ff, 0 0 20px rgba(0,212,255,0.3)",
  },
  {
    id: "neon_purple",
    label: "Neon Purple",
    border: "2px solid #a855f7",
    shadow: "0 0 10px #a855f7, 0 0 20px rgba(168,85,247,0.3)",
  },
  {
    id: "neon_green",
    label: "Neon Green",
    border: "2px solid #22c55e",
    shadow: "0 0 10px #22c55e, 0 0 20px rgba(34,197,94,0.3)",
  },
  {
    id: "rainbow",
    label: "Rainbow",
    border: "3px solid transparent",
    shadow: "0 0 8px rgba(168,85,247,0.4)",
    outline: "none",
  },
  {
    id: "double",
    label: "Double Ring",
    border: "3px solid rgba(255,255,255,0.6)",
    outline: "2px solid rgba(255,255,255,0.3)",
  },
  {
    id: "custom",
    label: "Custom…",
    border: "",
  },
];
