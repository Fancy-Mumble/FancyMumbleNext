import {
  register,
  unregister,
  isRegistered,
} from "@tauri-apps/plugin-global-shortcut";
import { invoke } from "@tauri-apps/api/core";
import { load } from "@tauri-apps/plugin-store";

export interface ShortcutBindings {
  toggleMute: string;
  toggleDeafen: string;
}

const SHORTCUT_STORE = "shortcuts.json";

export async function loadShortcuts(): Promise<ShortcutBindings> {
  const store = await load(SHORTCUT_STORE, { autoSave: true, defaults: {} });
  const saved = await store.get<ShortcutBindings>("shortcuts");
  return saved ?? { toggleMute: "", toggleDeafen: "" };
}

export async function saveShortcuts(shortcuts: ShortcutBindings): Promise<void> {
  const store = await load(SHORTCUT_STORE, { autoSave: true, defaults: {} });
  await store.set("shortcuts", shortcuts);
}

export function eventToShortcut(e: React.KeyboardEvent): string | null {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Super");
  const key = e.key;
  if (["Control", "Alt", "Shift", "Meta"].includes(key)) return null;
  parts.push(key.length === 1 ? key.toUpperCase() : key);
  return parts.join("+");
}

export async function applyGlobalShortcut(
  shortcut: string,
  command: string,
): Promise<void> {
  if (!shortcut) return;
  try {
    if (await isRegistered(shortcut)) await unregister(shortcut);
    await register(shortcut, (event) => {
      if (event.state === "Pressed") {
        invoke(command).catch(console.error);
      }
    });
  } catch (e) {
    console.warn(`Failed to register shortcut "${shortcut}":`, e);
  }
}

export async function clearGlobalShortcut(shortcut: string): Promise<void> {
  if (!shortcut) return;
  try {
    if (await isRegistered(shortcut)) await unregister(shortcut);
  } catch {
    /* ignore */
  }
}
