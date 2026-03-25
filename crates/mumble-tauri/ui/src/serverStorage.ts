/**
 * Persistent storage for saved Mumble server connections
 * using `@tauri-apps/plugin-store` (Tauri Store v2).
 *
 * Server metadata lives in `servers.json`.
 * Passwords are stored separately in `passwords.json` keyed by server id.
 */

import { load } from "@tauri-apps/plugin-store";
import type { SavedServer } from "./types";

const STORE_FILE = "servers.json";
const KEY = "servers";

const PASSWORD_STORE_FILE = "passwords.json";
const PASSWORD_KEY = "passwords";

/** Map of server-id to password. */
type PasswordMap = Record<string, string>;

async function getStore() {
  return load(STORE_FILE, { autoSave: true, defaults: {} });
}

async function getPasswordStore() {
  return load(PASSWORD_STORE_FILE, { autoSave: true, defaults: {} });
}

/** Return all saved servers (newest first). */
export async function getSavedServers(): Promise<SavedServer[]> {
  const store = await getStore();
  const servers = await store.get<SavedServer[]>(KEY);
  // Normalize legacy entries that may not have cert_label or favorite.
  return (servers ?? []).map((s) => ({ ...s, cert_label: s.cert_label ?? null, favorite: s.favorite ?? false }));
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

/** Remove a saved server by id. Also removes any stored password. */
export async function removeServer(id: string): Promise<void> {
  const store = await getStore();
  const servers = (await getSavedServers()).filter((s) => s.id !== id);
  await store.set(KEY, servers);
  await removeServerPassword(id);
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

// -- Password storage ----------------------------------------------

/** Retrieve the stored password for a server, or null if none saved. */
export async function getServerPassword(serverId: string): Promise<string | null> {
  const store = await getPasswordStore();
  const map = (await store.get<PasswordMap>(PASSWORD_KEY)) ?? {};
  return map[serverId] ?? null;
}

/** Save a password for a server. Pass null to remove it. */
export async function setServerPassword(serverId: string, password: string | null): Promise<void> {
  const store = await getPasswordStore();
  const map = (await store.get<PasswordMap>(PASSWORD_KEY)) ?? {};
  if (password) {
    map[serverId] = password;
  } else {
    delete map[serverId];
  }
  await store.set(PASSWORD_KEY, map);
}

/** Remove any stored password for a server. */
export async function removeServerPassword(serverId: string): Promise<void> {
  await setServerPassword(serverId, null);
}
