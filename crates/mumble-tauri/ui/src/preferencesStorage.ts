/**
 * Persistent storage for user preferences (app-wide settings)
 * using `@tauri-apps/plugin-store` (Tauri Store v2).
 */

import { load } from "@tauri-apps/plugin-store";
import type { AudioSettings, UserPreferences, UserMode } from "./types";

const STORE_FILE = "preferences.json";
const KEY = "preferences";
const AUDIO_KEY = "audioSettings";

const DEFAULTS: UserPreferences = {
  userMode: "normal",
  hasCompletedSetup: false,
  defaultUsername: "",
  klipyApiKey: "",
  timeFormat: "auto",
  convertToLocalTime: true,
  enableNotifications: true,
};

async function getStore() {
  return load(STORE_FILE, { autoSave: true, defaults: {} });
}

/** Return the current user preferences, falling back to defaults. */
export async function getPreferences(): Promise<UserPreferences> {
  const store = await getStore();
  const prefs = await store.get<UserPreferences>(KEY);
  return prefs ? { ...DEFAULTS, ...prefs } : { ...DEFAULTS };
}

/** Persist the full preferences object. */
export async function setPreferences(
  prefs: UserPreferences,
): Promise<void> {
  const store = await getStore();
  await store.set(KEY, prefs);
}

/** Update specific preference fields. */
export async function updatePreferences(
  patch: Partial<UserPreferences>,
): Promise<UserPreferences> {
  const current = await getPreferences();
  const updated = { ...current, ...patch };
  await setPreferences(updated);
  return updated;
}

/** Check whether this is the user's first run (setup not completed). */
export async function isFirstRun(): Promise<boolean> {
  const prefs = await getPreferences();
  return !prefs.hasCompletedSetup;
}

/** Get the stored user mode. */
export async function getUserMode(): Promise<UserMode> {
  const prefs = await getPreferences();
  return prefs.userMode;
}

/** Get the stored default username. */
export async function getDefaultUsername(): Promise<string> {
  const prefs = await getPreferences();
  return prefs.defaultUsername;
}

/** Finalise first-run setup by storing mode, default username, and marking complete. */
export async function completeSetup(
  mode: UserMode,
  defaultUsername: string,
): Promise<void> {
  await updatePreferences({
    userMode: mode,
    defaultUsername,
    hasCompletedSetup: true,
  });
}

// -- Audio settings persistence ------------------------------------

/** Return persisted audio settings, or null if none saved yet. */
export async function getSavedAudioSettings(): Promise<AudioSettings | null> {
  const store = await getStore();
  return (await store.get<AudioSettings>(AUDIO_KEY)) ?? null;
}

/** Persist audio settings to disk. */
export async function saveAudioSettings(
  settings: AudioSettings,
): Promise<void> {
  const store = await getStore();
  await store.set(AUDIO_KEY, settings);
}

// -- Dismissed persistence banners ---------------------------------

const DISMISSED_BANNERS_KEY = "dismissedPersistenceBanners";

/** Return channel IDs whose persistence banner was dismissed. */
export async function getDismissedBanners(): Promise<number[]> {
  const store = await getStore();
  return (await store.get<number[]>(DISMISSED_BANNERS_KEY)) ?? [];
}

/** Mark a channel's persistence banner as dismissed. */
export async function dismissBanner(channelId: number): Promise<void> {
  const store = await getStore();
  const current = (await store.get<number[]>(DISMISSED_BANNERS_KEY)) ?? [];
  if (!current.includes(channelId)) {
    await store.set(DISMISSED_BANNERS_KEY, [...current, channelId]);
  }
}

// -- Silenced channels (per-server, local-only) --------------------

/**
 * Silenced channels are keyed by server address ("host:port") so each
 * server has its own independent blacklist.
 */
const SILENCED_CHANNELS_KEY = "silencedChannels";

type SilencedMap = Record<string, number[]>;

/** Return the channel IDs silenced for a given server. */
export async function getSilencedChannels(
  serverKey: string,
): Promise<number[]> {
  const store = await getStore();
  const map = (await store.get<SilencedMap>(SILENCED_CHANNELS_KEY)) ?? {};
  return map[serverKey] ?? [];
}

/** Toggle the silenced state for a single channel on a server. */
export async function setSilencedChannel(
  serverKey: string,
  channelId: number,
  silenced: boolean,
): Promise<number[]> {
  const store = await getStore();
  const map = (await store.get<SilencedMap>(SILENCED_CHANNELS_KEY)) ?? {};
  const current = map[serverKey] ?? [];
  let updated: number[];
  if (silenced && !current.includes(channelId)) {
    updated = [...current, channelId];
  } else if (!silenced) {
    updated = current.filter((id) => id !== channelId);
  } else {
    return current;
  }
  map[serverKey] = updated;
  await store.set(SILENCED_CHANNELS_KEY, map);
  return updated;
}
