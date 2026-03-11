/**
 * Persistent storage for saved Mumble server connections
 * using `@tauri-apps/plugin-store` (Tauri Store v2).
 */

import { load } from "@tauri-apps/plugin-store";
import type { SavedServer } from "./types";

const STORE_FILE = "servers.json";
const KEY = "servers";

async function getStore() {
  return load(STORE_FILE, { autoSave: true, defaults: {} });
}

/** Return all saved servers (newest first). */
export async function getSavedServers(): Promise<SavedServer[]> {
  const store = await getStore();
  const servers = await store.get<SavedServer[]>(KEY);
  // Normalize legacy entries that may not have cert_label.
  return (servers ?? []).map((s) => ({ ...s, cert_label: s.cert_label ?? null }));
}

/** Persist a new server entry. Returns the created entry. */
export async function addServer(
  server: Omit<SavedServer, "id">,
): Promise<SavedServer> {
  const store = await getStore();
  const servers = await getSavedServers();
  const entry: SavedServer = { ...server, id: crypto.randomUUID() };
  servers.unshift(entry); // newest first
  await store.set(KEY, servers);
  return entry;
}

/** Remove a saved server by id. */
export async function removeServer(id: string): Promise<void> {
  const store = await getStore();
  const servers = (await getSavedServers()).filter((s) => s.id !== id);
  await store.set(KEY, servers);
}

/** Update an existing server entry. */
export async function updateServer(
  id: string,
  patch: Partial<Omit<SavedServer, "id">>,
): Promise<void> {
  const store = await getStore();
  const servers = await getSavedServers();
  const idx = servers.findIndex((s) => s.id === id);
  if (idx !== -1) {
    servers[idx] = { ...servers[idx], ...patch };
    await store.set(KEY, servers);
  }
}
