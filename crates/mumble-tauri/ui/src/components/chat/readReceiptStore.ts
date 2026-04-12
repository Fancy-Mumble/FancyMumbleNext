/**
 * In-memory read receipt store.
 *
 * Read receipts arrive via FancyReadReceiptDeliver (wire ID 127).
 * Each entry tracks a user's read watermark (last_read_message_id)
 * for a channel, keyed by cert_hash.
 */

import type { ReadState } from "../../types";

// -- Module-level store -------------------------------------------

/**
 * channelId -> cert_hash -> ReadState
 *
 * Mutable maps for performance; components trigger re-renders via
 * Zustand `setState({})` after mutations (same pattern as reactions).
 */
const readStateMap = new Map<number, Map<string, ReadState>>();

// -- Accessors ----------------------------------------------------

/** Get all read states for a channel. */
export function getChannelReadStates(channelId: number): ReadState[] {
  const byHash = readStateMap.get(channelId);
  if (!byHash) return [];
  return [...byHash.values()];
}

/** Check whether a specific user (by cert_hash) has read at least up to a given message. */
export function hasUserReadMessage(
  channelId: number,
  certHash: string,
  messageId: string,
  allMessageIds: string[],
): boolean {
  const byHash = readStateMap.get(channelId);
  if (!byHash) return false;
  const state = byHash.get(certHash);
  if (!state) return false;

  const watermarkIdx = allMessageIds.indexOf(state.last_read_message_id);
  const messageIdx = allMessageIds.indexOf(messageId);
  if (watermarkIdx === -1 || messageIdx === -1) return false;
  return watermarkIdx >= messageIdx;
}

/**
 * Get the list of users who have read a specific message in a channel.
 * `allMessageIds` is the ordered list of message IDs in the channel
 * (oldest first) used to compare watermark positions.
 */
export function getReadersForMessage(
  channelId: number,
  messageId: string,
  allMessageIds: string[],
): ReadState[] {
  const byHash = readStateMap.get(channelId);
  if (!byHash) return [];

  const targetIdx = allMessageIds.indexOf(messageId);
  if (targetIdx === -1) return [];

  const readers: ReadState[] = [];
  for (const state of byHash.values()) {
    const watermarkIdx = allMessageIds.indexOf(state.last_read_message_id);
    if (watermarkIdx >= targetIdx) {
      readers.push(state);
    }
  }
  return readers;
}

/**
 * Check if ALL active channel users (excluding the sender) have read
 * a given message. Used for the double-checkmark indicator.
 */
export function allActiveUsersRead(
  channelId: number,
  messageId: string,
  allMessageIds: string[],
  activeUserHashes: string[],
  ownCertHash: string | undefined,
): boolean {
  if (activeUserHashes.length === 0) return false;
  const othersHashes = ownCertHash
    ? activeUserHashes.filter((h) => h !== ownCertHash)
    : activeUserHashes;
  if (othersHashes.length === 0) return true;

  const targetIdx = allMessageIds.indexOf(messageId);
  if (targetIdx === -1) return false;

  const byHash = readStateMap.get(channelId);
  if (!byHash) return false;

  return othersHashes.every((hash) => {
    const state = byHash.get(hash);
    if (!state) return false;
    const watermarkIdx = allMessageIds.indexOf(state.last_read_message_id);
    return watermarkIdx >= targetIdx;
  });
}

// -- Mutations ----------------------------------------------------

/** Apply incoming read states (from a FancyReadReceiptDeliver event). */
export function applyReadStates(channelId: number, states: ReadState[]): void {
  let byHash = readStateMap.get(channelId);
  if (!byHash) {
    byHash = new Map();
    readStateMap.set(channelId, byHash);
  }
  for (const rs of states) {
    if (!rs.cert_hash) continue;
    const existing = byHash.get(rs.cert_hash);
    if (!existing || rs.timestamp >= existing.timestamp) {
      byHash.set(rs.cert_hash, rs);
    }
  }
}

/** Clear all read receipt data (e.g. on disconnect). */
export function clearReadReceipts(): void {
  readStateMap.clear();
}
