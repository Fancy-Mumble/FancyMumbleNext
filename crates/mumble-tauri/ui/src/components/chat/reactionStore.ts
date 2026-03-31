/**
 * In-memory reaction store, analogous to the poll module-level stores.
 *
 * Reactions are transmitted via PluginDataTransmission (data_id "fancy-reaction")
 * and are NOT persisted across sessions.
 *
 * Wire format (JSON):
 * ```json
 * { "type": "reaction", "messageId": "...", "emoji": "...", "action": "add"|"remove",
 *   "reactor": 42, "reactorName": "Alice", "channelId": 5 }
 * ```
 */

// -- Payload types ------------------------------------------------

export interface ReactionPayload {
  readonly type: "reaction";
  /** Message being reacted to. */
  readonly messageId: string;
  /** Emoji string (single grapheme, e.g. unicode emoji or server shortcode). */
  readonly emoji: string;
  /** Whether to add or retract the reaction. */
  readonly action: "add" | "remove";
  /** Reactor's session ID. */
  readonly reactor: number;
  /** Reactor's display name (resolved by sender). */
  readonly reactorName: string;
  /** Channel the message belongs to (used for scoping). */
  readonly channelId: number;
}

/** Compact per-emoji summary exposed to rendering components. */
export interface ReactionSummary {
  readonly emoji: string;
  /** Session IDs of all users who reacted with this emoji. */
  readonly reactors: ReadonlySet<number>;
  /** Display names, best-effort resolved. */
  readonly reactorNames: ReadonlyMap<number, string>;
  /** Cert hashes of all users who reacted (for persistent channel reactions). */
  readonly reactorHashes: ReadonlySet<string>;
  /** Display names keyed by cert hash (persistent channel reactions). */
  readonly reactorHashNames: ReadonlyMap<string, string>;
  /** Timestamp of the first reaction of this emoji kind (used for display ordering). */
  readonly firstTimestamp: number;
}

// -- Server custom reactions --------------------------------------

export interface ServerCustomReaction {
  /** Short-code identifier, e.g. ":mumble:" */
  readonly shortcode: string;
  /** Display string (emoji char, image URL, or unicode). */
  readonly display: string;
  /** Optional human-readable label. */
  readonly label?: string;
}

/**
 * Plugin data id for server-advertised custom reactions.
 * Servers broadcast this on connect; clients cache it.
 */
export const CUSTOM_REACTIONS_DATA_ID = "fancy-custom-reactions";
export const REACTION_DATA_ID = "fancy-reaction";

// -- Module-level store -------------------------------------------

/**
 * messageId -> emoji -> { sessions, names, hashes, hashNames, firstTimestamp }
 *
 * Mutable maps for performance; components trigger re-renders via
 * Zustand `setState({})` after mutations (same pattern as polls).
 */
const reactionMap = new Map<string, Map<string, {
  sessions: Set<number>;
  names: Map<number, string>;
  hashes: Set<string>;
  hashNames: Map<string, string>;
  firstTimestamp: number;
}>>();

/** Server-provided custom reactions for the current connection. */
let serverCustomReactions: ServerCustomReaction[] = [];

// -- Accessors ----------------------------------------------------

/** Get all reaction summaries for a specific message, ordered by first-reaction timestamp. */
export function getReactions(messageId: string): ReactionSummary[] {
  const byEmoji = reactionMap.get(messageId);
  if (!byEmoji) return [];
  const result: ReactionSummary[] = [];
  for (const [emoji, data] of byEmoji) {
    if (data.sessions.size === 0 && data.hashes.size === 0) continue;
    result.push({
      emoji,
      reactors: data.sessions,
      reactorNames: data.names,
      reactorHashes: data.hashes,
      reactorHashNames: data.hashNames,
      firstTimestamp: data.firstTimestamp,
    });
  }
  // Sort by first-reaction timestamp so the first emoji reacted with stays first.
  result.sort((a, b) => a.firstTimestamp - b.firstTimestamp);
  return result;
}

/** Check whether a specific session has reacted with an emoji on a message. */
export function hasReacted(messageId: string, emoji: string, session: number): boolean {
  return reactionMap.get(messageId)?.get(emoji)?.sessions.has(session) ?? false;
}

/** Get the current list of server custom reactions. */
export function getServerCustomReactions(): ServerCustomReaction[] {
  return serverCustomReactions;
}

// -- Mutations ----------------------------------------------------

/** Apply a reaction payload (local or remote, session-based). */
export function applyReaction(payload: ReactionPayload): void {
  let byEmoji = reactionMap.get(payload.messageId);
  if (!byEmoji) {
    byEmoji = new Map();
    reactionMap.set(payload.messageId, byEmoji);
  }

  let data = byEmoji.get(payload.emoji);
  if (!data) {
    data = { sessions: new Set(), names: new Map(), hashes: new Set(), hashNames: new Map(), firstTimestamp: Date.now() };
    byEmoji.set(payload.emoji, data);
  }

  if (payload.action === "add") {
    data.sessions.add(payload.reactor);
    data.names.set(payload.reactor, payload.reactorName);
  } else {
    data.sessions.delete(payload.reactor);
    data.names.delete(payload.reactor);
    // Clean up empty entries.
    if (data.sessions.size === 0 && data.hashes.size === 0) byEmoji.delete(payload.emoji);
    if (byEmoji.size === 0) reactionMap.delete(payload.messageId);
  }
}

/** Apply a persistent-channel reaction (cert-hash-based, from PchatReactionDeliver). */
export function applyPchatReaction(
  messageId: string,
  emoji: string,
  action: "add" | "remove",
  senderHash: string,
  senderName: string,
): void {
  let byEmoji = reactionMap.get(messageId);
  if (!byEmoji) {
    byEmoji = new Map();
    reactionMap.set(messageId, byEmoji);
  }

  let data = byEmoji.get(emoji);
  if (!data) {
    data = { sessions: new Set(), names: new Map(), hashes: new Set(), hashNames: new Map(), firstTimestamp: Date.now() };
    byEmoji.set(emoji, data);
  }

  if (action === "add") {
    data.hashes.add(senderHash);
    data.hashNames.set(senderHash, senderName);
  } else {
    data.hashes.delete(senderHash);
    data.hashNames.delete(senderHash);
    if (data.sessions.size === 0 && data.hashes.size === 0) byEmoji.delete(emoji);
    if (byEmoji.size === 0) reactionMap.delete(messageId);
  }
}

/** Check whether a specific cert hash has reacted with an emoji on a message (pchat). */
export function hasReactedByHash(messageId: string, emoji: string, certHash: string): boolean {
  return reactionMap.get(messageId)?.get(emoji)?.hashes.has(certHash) ?? false;
}

/** Store server-advertised custom reactions. */
export function setServerCustomReactions(reactions: ServerCustomReaction[]): void {
  serverCustomReactions = reactions;
}

/** Clear all reaction data (called on disconnect). */
export function resetReactions(): void {
  reactionMap.clear();
  serverCustomReactions = [];
}
