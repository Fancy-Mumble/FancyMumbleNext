/**
 * Persistent storage for personalization settings (chat background, etc.)
 * using `@tauri-apps/plugin-store` (Tauri Store v2).
 */

import { load } from "@tauri-apps/plugin-store";
import type { ThemeId } from "./themes";

export type BubbleStyle = "bubbles" | "flat" | "compact";
export type FontSize = "small" | "medium" | "large";
export type BgFit = "cover" | "tile";
export type ChannelViewerStyle = "classic" | "flat" | "modern";

export interface PersonalizationData {
  /** Original (un-blurred) background image as data-URL, or null if none. */
  chatBgOriginal: string | null;
  /** Pre-blurred background image as data-URL, or null if blur is off / no image. */
  chatBgBlurred: string | null;
  /** Blur sigma value (0 = no blur). */
  chatBgBlurSigma: number;
  /** Background opacity (0.0 - 1.0). */
  chatBgOpacity: number;
  /** Background dim/overlay darkness (0.0 - 1.0). */
  chatBgDim: number;
  /** How the background image fills the chat area ("cover" or "tile"). */
  chatBgFit: BgFit;
  /** Message bubble visual style. */
  bubbleStyle: BubbleStyle;
  /** Font size preset (or custom px value stored as number). */
  fontSize: FontSize;
  /** Custom font size in pixels (used only when fontSize === "large" in expert mode). */
  fontSizeCustomPx: number;
  /** Font family for chat messages. */
  fontFamily: string;
  /** Compact mode — hide avatars and tighten spacing. */
  compactMode: boolean;
  /** Channel sidebar viewer style. */
  channelViewerStyle: ChannelViewerStyle;
  /** Active color theme. */
  theme: ThemeId;
}

const STORE_FILE = "personalization.json";
const KEY = "data";

const DEFAULTS: PersonalizationData = {
  chatBgOriginal: null,
  chatBgBlurred: null,
  chatBgBlurSigma: 0,
  chatBgOpacity: 0.25,
  chatBgDim: 0.5,
  chatBgFit: "cover",
  bubbleStyle: "bubbles",
  fontSize: "medium",
  fontSizeCustomPx: 14,
  fontFamily: "system",
  compactMode: false,
  channelViewerStyle: "flat",
  theme: "dark",
};

async function getStore() {
  return load(STORE_FILE, { autoSave: true, defaults: {} });
}

// In-flight + cached load promise so concurrent / repeat callers on
// startup share a single IPC roundtrip (the personalization payload can
// include large image data URLs and cost ~200 KiB per fetch).
let cachedLoad: Promise<PersonalizationData> | null = null;

/** Return persisted personalization data, falling back to defaults. */
export async function loadPersonalization(): Promise<PersonalizationData> {
  if (cachedLoad) return cachedLoad;
  cachedLoad = (async () => {
    const store = await getStore();
    const data = await store.get<PersonalizationData>(KEY);
    return data ? { ...DEFAULTS, ...data } : { ...DEFAULTS };
  })();
  try {
    return await cachedLoad;
  } catch (e) {
    cachedLoad = null;
    throw e;
  }
}

/** Persist personalization data. */
export async function savePersonalization(
  data: PersonalizationData,
): Promise<void> {
  const store = await getStore();
  await store.set(KEY, data);
  cachedLoad = Promise.resolve(data);
}
