/**
 * Self-contained API wrapper for the Tauri auto-updater bootstrapper.
 *
 * Only the `UpdaterWindow` component should import from this file.
 * Everything in this folder is intentionally decoupled from the rest
 * of the application (no shared store, no shared components).
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { load } from "@tauri-apps/plugin-store";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type UpdateInfo = {
  version: string;
  current_version: string;
  date: string | null;
  body: string | null;
};

export type ProgressEvent =
  | { kind: "started"; total: number | null }
  | { kind: "chunk"; downloaded: number; total: number | null }
  | { kind: "finished" };

const PROGRESS_EVENT = "updater://progress";
const PREFS_FILE = "preferences.json";
const PREFS_KEY = "preferences";

/**
 * Persist (or clear) the "skip this version" choice. Updates both the
 * shared `preferences.json` plugin-store file - so the main app picks
 * it up on next launch - and the in-process Rust updater state, so the
 * change takes effect immediately (although typically the updater
 * window is closed right after).
 */
async function persistSkippedVersion(version: string | null): Promise<void> {
  try {
    const store = await load(PREFS_FILE, { autoSave: true, defaults: {} });
    const current = (await store.get<Record<string, unknown>>(PREFS_KEY)) ?? {};
    await store.set(PREFS_KEY, { ...current, skippedUpdateVersion: version });
  } catch (e) {
    console.warn("persistSkippedVersion: store update failed", e);
  }
  try {
    await invoke("updater_set_skipped_version", { version });
  } catch (e) {
    console.warn("persistSkippedVersion: backend update failed", e);
  }
}

export const updaterApi = {
  pending: () => invoke<UpdateInfo | null>("updater_pending"),
  check: () => invoke<UpdateInfo | null>("updater_check"),
  install: () => invoke<void>("updater_download_and_install"),
  dismiss: () => invoke<void>("updater_dismiss"),
  onProgress: (cb: (e: ProgressEvent) => void): Promise<UnlistenFn> =>
    listen<ProgressEvent>(PROGRESS_EVENT, (event) => cb(event.payload)),
  closeWindow: () => getCurrentWindow().close().catch(() => undefined),
  setSkippedVersion: persistSkippedVersion,
};

/**
 * Detect whether the current webview was launched as the dedicated
 * updater bootstrapper window (via `?updater=1` in the URL).
 */
export function isUpdaterWindow(): boolean {
  return new URLSearchParams(globalThis.location.search).has("updater");
}

/**
 * Returns true when the updater window was opened with `?auto=1`,
 * meaning the bootstrapper should immediately start downloading and
 * installing the pending update without waiting for the user.
 */
export function isAutoInstall(): boolean {
  return new URLSearchParams(globalThis.location.search).has("auto");
}
