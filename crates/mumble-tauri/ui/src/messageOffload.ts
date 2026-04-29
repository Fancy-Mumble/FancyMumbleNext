/**
 * Message content offloading abstraction.
 *
 * Provides a generic `MessageContentProvider` interface that decouples
 * the "what" (store / retrieve heavy message content) from the "how"
 * (encrypted local temp files, future server-side persistent storage,
 * etc.).
 *
 * The default implementation, `LocalEncryptedProvider`, delegates to
 * Tauri commands that encrypt content with a per-session ChaCha20-Poly1305
 * key and write it to temp files.
 */

import { invoke } from "@tauri-apps/api/core";

// --- Abstraction --------------------------------------------------

/** Scope that identifies where a message lives (channel / DM). */
export interface MessageScope {
  scope: "channel" | "dm";
  scopeId: string;
}

/**
 * Generic interface for storing and retrieving heavy message content.
 *
 * Implementations may persist data locally (encrypted temp files) or
 * remotely (e.g. server-side message history).
 */
export interface MessageContentProvider {
  /** Store content under the given key.  May be a no-op for server-backed providers. */
  store(key: string, content: string, ctx: MessageScope): Promise<void>;
  /** Retrieve previously stored content.  Returns `null` if unavailable. */
  retrieve(key: string, ctx: MessageScope): Promise<string | null>;
  /**
   * Retrieve multiple keys in a single call.
   *
   * Returns a map of key to content.  Keys that failed to load are
   * omitted.  The default implementation calls `retrieve` in parallel.
   */
  retrieveMany(keys: string[], ctx: MessageScope): Promise<Record<string, string>>;
  /** Hint that the content for `key` is no longer needed locally. */
  release(key: string): Promise<void>;
  /** Release all stored content (e.g. on disconnect). */
  dispose(): Promise<void>;
}

// --- Local encrypted implementation (Tauri commands) --------------

/**
 * Stores message bodies in encrypted temp files via the Rust backend.
 *
 * The encryption key is held only in memory in the Rust process and
 * is never persisted.  Each file uses a unique random nonce for
 * ChaCha20-Poly1305 AEAD.
 */
class LocalEncryptedProvider implements MessageContentProvider {
  async store(key: string, _content: string, ctx: MessageScope): Promise<void> {
    await invoke("offload_message", {
      messageId: key,
      scope: ctx.scope,
      scopeId: ctx.scopeId,
    });
  }

  async retrieve(key: string, ctx: MessageScope): Promise<string | null> {
    try {
      return await invoke<string>("load_offloaded_message", {
        messageId: key,
        scope: ctx.scope,
        scopeId: ctx.scopeId,
      });
    } catch {
      return null;
    }
  }

  async retrieveMany(keys: string[], ctx: MessageScope): Promise<Record<string, string>> {
    try {
      return await invoke<Record<string, string>>("load_offloaded_messages_batch", {
        messageIds: keys,
        scope: ctx.scope,
        scopeId: ctx.scopeId,
      });
    } catch {
      return {};
    }
  }

  async release(_key: string): Promise<void> {
    // Individual file cleanup happens on retrieve (Rust side removes
    // the file after restoring the in-memory body).
  }

  async dispose(): Promise<void> {
    await invoke("clear_offloaded_messages");
  }
}

// --- Offload helpers ----------------------------------------------

const OFFLOAD_PREFIX = "<!-- OFFLOADED:";
const OFFLOAD_SUFFIX = " -->";

/** Minimum body length (bytes) to consider a message "heavy". */
const HEAVY_THRESHOLD = 4096;

/** Regex that matches embedded data-URL sources for images and videos. */
const DATA_URL_RE = /src="data:(image|video)\//;

/** Whether a message body contains heavy inline content worth offloading. */
export function isHeavyContent(body: string): boolean {
  return body.length > HEAVY_THRESHOLD && DATA_URL_RE.test(body);
}

/** Whether a message body is an offload placeholder. */
export function isOffloaded(body: string): boolean {
  return body.startsWith(OFFLOAD_PREFIX);
}

/** Build the lightweight placeholder for an offloaded message. */
export function offloadPlaceholder(messageId: string, contentLength: number): string {
  return `${OFFLOAD_PREFIX}${messageId}:${contentLength}${OFFLOAD_SUFFIX}`;
}

/**
 * Extract the message key and original content byte-length from an
 * offload placeholder.  Returns `null` for non-placeholder strings.
 */
export function extractOffloadInfo(body: string): { key: string; contentLength: number } | null {
  if (!body.startsWith(OFFLOAD_PREFIX)) return null;
  const end = body.indexOf(OFFLOAD_SUFFIX, OFFLOAD_PREFIX.length);
  if (end === -1) return null;
  const inner = body.slice(OFFLOAD_PREFIX.length, end);
  const colonIdx = inner.lastIndexOf(":");
  if (colonIdx === -1) {
    // Legacy placeholder without size.
    return { key: inner, contentLength: 0 };
  }
  const key = inner.slice(0, colonIdx);
  const contentLength = Number.parseInt(inner.slice(colonIdx + 1), 10) || 0;
  return { key, contentLength };
}

// --- Offload manager ----------------------------------------------

/** Delay (ms) before a message leaving the viewport is actually offloaded. */
const OFFLOAD_DELAY_MS = 5_000;

/**
 * Coordinates offloading and restoring of heavy message content.
 *
 * The manager keeps track of which messages are currently offloaded or
 * in-flight (loading), and provides debounced scheduling so rapid
 * scroll movements don't cause unnecessary encrypt/decrypt churn.
 */
export class MessageOffloadManager {
  private provider: MessageContentProvider;
  /** Message IDs that are currently offloaded to the provider. */
  private readonly _offloaded = new Set<string>();
  /** Message IDs that are currently being loaded (decrypt in-flight). */
  private readonly _loading = new Set<string>();
  /** Pending offload timers keyed by message ID. */
  private readonly pendingOffloads = new Map<string, ReturnType<typeof setTimeout>>();

  constructor(provider: MessageContentProvider) {
    this.provider = provider;
  }

  /** Replace the underlying provider (e.g. switch to server storage). */
  setProvider(provider: MessageContentProvider): void {
    this.provider = provider;
  }

  /** Whether the given message ID is currently offloaded. */
  isOffloaded(id: string): boolean {
    return this._offloaded.has(id);
  }

  /** Whether the given message ID is currently being restored. */
  isLoading(id: string): boolean {
    return this._loading.has(id);
  }

  /**
   * Schedule offloading a message after a delay.
   *
   * If the message scrolls back into view before the delay elapses,
   * call `cancelOffload` to prevent the write.
   */
  scheduleOffload(
    messageId: string,
    ctx: MessageScope,
    onOffloaded: () => void,
  ): void {
    if (this._offloaded.has(messageId) || this.pendingOffloads.has(messageId)) return;

    const timer = setTimeout(async () => {
      this.pendingOffloads.delete(messageId);
      try {
        await this.provider.store(messageId, "", ctx);
        this._offloaded.add(messageId);
        onOffloaded();
      } catch (e) {
        console.warn("offload failed:", e);
      }
    }, OFFLOAD_DELAY_MS);

    this.pendingOffloads.set(messageId, timer);
  }

  /** Cancel a pending offload (message came back into view). */
  cancelOffload(messageId: string): void {
    const timer = this.pendingOffloads.get(messageId);
    if (timer !== undefined) {
      clearTimeout(timer);
      this.pendingOffloads.delete(messageId);
    }
  }

  /**
   * Restore an offloaded message body from the provider.
   *
   * Returns the original body, or `null` if retrieval failed.
   */
  async restore(
    messageId: string,
    ctx: MessageScope,
  ): Promise<string | null> {
    if (!this._offloaded.has(messageId)) return null;
    this._loading.add(messageId);

    try {
      const body = await this.provider.retrieve(messageId, ctx);
      this._offloaded.delete(messageId);
      return body;
    } catch (e) {
      console.warn("restore failed:", e);
      return null;
    } finally {
      this._loading.delete(messageId);
    }
  }

  /**
   * Restore multiple offloaded messages in a single IPC call.
   *
   * Returns a map of message_id to restored body.  Failed keys are
   * omitted.  This is more efficient than calling `restore` in a loop
   * because it batches all decryption into one Rust-side lock acquire.
   */
  async restoreMany(
    messageIds: string[],
    ctx: MessageScope,
  ): Promise<Record<string, string>> {
    const toRestore = messageIds.filter((id) => this._offloaded.has(id) && !this._loading.has(id));
    if (toRestore.length === 0) return {};

    for (const id of toRestore) this._loading.add(id);

    try {
      const results = await this.provider.retrieveMany(toRestore, ctx);
      for (const id of toRestore) {
        if (id in results) this._offloaded.delete(id);
      }
      return results;
    } catch (e) {
      console.warn("batch restore failed:", e);
      return {};
    } finally {
      for (const id of toRestore) this._loading.delete(id);
    }
  }

  /** Clear all state and release provider resources. */
  async dispose(): Promise<void> {
    for (const timer of this.pendingOffloads.values()) {
      clearTimeout(timer);
    }
    this.pendingOffloads.clear();
    this._offloaded.clear();
    this._loading.clear();
    await this.provider.dispose();
  }
}

// --- Singleton ----------------------------------------------------

/** Global offload manager instance (local encrypted provider). */
export const offloadManager = new MessageOffloadManager(
  new LocalEncryptedProvider(),
);
