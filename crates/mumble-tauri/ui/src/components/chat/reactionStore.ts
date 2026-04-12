/**
 * In-memory reaction store, analogous to the poll module-level stores.
 *
 * Reactions arrive via PchatReactionDeliver (wire ID 118) for both
 * persistent and non-persistent channels. They are identified by the
 * sender's TLS certificate hash.
 */

// -- Payload types ------------------------------------------------

/** Compact per-emoji summary exposed to rendering components. */
export interface ReactionSummary {
  readonly emoji: string;
  /** Cert hashes of all users who reacted with this emoji. */
  readonly reactorHashes: ReadonlySet<string>;
  /** Display names keyed by cert hash. */
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

// -- Module-level store -------------------------------------------

/**
 * messageId -> emoji -> { hashes, hashNames, firstTimestamp }
 *
 * Mutable maps for performance; components trigger re-renders via
 * Zustand `setState({})` after mutations (same pattern as polls).
 */
const reactionMap = new Map<string, Map<string, {
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
    if (data.hashes.size === 0) continue;
    result.push({
      emoji,
      reactorHashes: data.hashes,
      reactorHashNames: data.hashNames,
      firstTimestamp: data.firstTimestamp,
    });
  }
  result.sort((a, b) => a.firstTimestamp - b.firstTimestamp);
  return result;
}

/** Check whether a specific cert hash has reacted with an emoji on a message. */
export function hasReacted(messageId: string, emoji: string, certHash: string): boolean {
  return reactionMap.get(messageId)?.get(emoji)?.hashes.has(certHash) ?? false;
}

/** Get the current list of server custom reactions. */
export function getServerCustomReactions(): ServerCustomReaction[] {
  return serverCustomReactions;
}

// -- Mutations ----------------------------------------------------

/** Apply a reaction event (from PchatReactionDeliver). */
export function applyReaction(
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
    data = { hashes: new Set(), hashNames: new Map(), firstTimestamp: Date.now() };
    byEmoji.set(emoji, data);
  }

  if (action === "add") {
    data.hashes.add(senderHash);
    data.hashNames.set(senderHash, senderName);
  } else {
    data.hashes.delete(senderHash);
    data.hashNames.delete(senderHash);
    if (data.hashes.size === 0) byEmoji.delete(emoji);
    if (byEmoji.size === 0) reactionMap.delete(messageId);
  }
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
