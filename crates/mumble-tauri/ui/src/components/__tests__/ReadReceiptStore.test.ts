/**
 * Unit tests for the module-level read receipt store.
 *
 * Tests cover: applying read states, watermark comparison, querying
 * readers for a message, allActiveUsersRead logic, and reset.
 */

import { describe, it, expect, beforeEach } from "vitest";
import {
  applyReadStates,
  clearReadReceipts,
  getChannelReadStates,
  hasUserReadMessage,
  getReadersForMessage,
  allActiveUsersRead,
} from "../chat/readReceiptStore";
import type { ReadState } from "../../types";

const CHANNEL = 1;
const MSG_IDS = ["msg-1", "msg-2", "msg-3", "msg-4"];

function makeReadState(certHash: string, name: string, lastRead: string, timestamp = Date.now()): ReadState {
  return { cert_hash: certHash, name, is_online: true, last_read_message_id: lastRead, timestamp };
}

beforeEach(() => {
  clearReadReceipts();
});

describe("applyReadStates + getChannelReadStates", () => {
  it("stores a single read state and returns it", () => {
    applyReadStates(CHANNEL, [makeReadState("hash-alice", "Alice", "msg-2")]);
    const states = getChannelReadStates(CHANNEL);
    expect(states).toHaveLength(1);
    expect(states[0].cert_hash).toBe("hash-alice");
    expect(states[0].last_read_message_id).toBe("msg-2");
  });

  it("stores multiple users", () => {
    applyReadStates(CHANNEL, [
      makeReadState("hash-alice", "Alice", "msg-2"),
      makeReadState("hash-bob", "Bob", "msg-3"),
    ]);
    const states = getChannelReadStates(CHANNEL);
    expect(states).toHaveLength(2);
  });

  it("updates watermark when newer timestamp arrives", () => {
    applyReadStates(CHANNEL, [makeReadState("hash-alice", "Alice", "msg-1", 1000)]);
    applyReadStates(CHANNEL, [makeReadState("hash-alice", "Alice", "msg-3", 2000)]);
    const states = getChannelReadStates(CHANNEL);
    expect(states).toHaveLength(1);
    expect(states[0].last_read_message_id).toBe("msg-3");
  });

  it("ignores older timestamp", () => {
    applyReadStates(CHANNEL, [makeReadState("hash-alice", "Alice", "msg-3", 2000)]);
    applyReadStates(CHANNEL, [makeReadState("hash-alice", "Alice", "msg-1", 1000)]);
    const states = getChannelReadStates(CHANNEL);
    expect(states[0].last_read_message_id).toBe("msg-3");
  });

  it("returns empty for unknown channel", () => {
    expect(getChannelReadStates(999)).toEqual([]);
  });
});

describe("hasUserReadMessage", () => {
  it("returns true when watermark is at or past the message", () => {
    applyReadStates(CHANNEL, [makeReadState("hash-alice", "Alice", "msg-3")]);
    expect(hasUserReadMessage(CHANNEL, "hash-alice", "msg-1", MSG_IDS)).toBe(true);
    expect(hasUserReadMessage(CHANNEL, "hash-alice", "msg-2", MSG_IDS)).toBe(true);
    expect(hasUserReadMessage(CHANNEL, "hash-alice", "msg-3", MSG_IDS)).toBe(true);
  });

  it("returns false when watermark is before the message", () => {
    applyReadStates(CHANNEL, [makeReadState("hash-alice", "Alice", "msg-2")]);
    expect(hasUserReadMessage(CHANNEL, "hash-alice", "msg-3", MSG_IDS)).toBe(false);
    expect(hasUserReadMessage(CHANNEL, "hash-alice", "msg-4", MSG_IDS)).toBe(false);
  });

  it("returns false for unknown user", () => {
    applyReadStates(CHANNEL, [makeReadState("hash-alice", "Alice", "msg-3")]);
    expect(hasUserReadMessage(CHANNEL, "hash-unknown", "msg-1", MSG_IDS)).toBe(false);
  });
});

describe("getReadersForMessage", () => {
  it("returns users whose watermark is at or past the message", () => {
    applyReadStates(CHANNEL, [
      makeReadState("hash-alice", "Alice", "msg-3"),
      makeReadState("hash-bob", "Bob", "msg-1"),
      makeReadState("hash-charlie", "Charlie", "msg-4"),
    ]);
    const readers = getReadersForMessage(CHANNEL, "msg-2", MSG_IDS);
    const names = readers.map((r) => r.name).sort();
    expect(names).toEqual(["Alice", "Charlie"]);
  });

  it("returns empty when no one has read past the message", () => {
    applyReadStates(CHANNEL, [makeReadState("hash-bob", "Bob", "msg-1")]);
    expect(getReadersForMessage(CHANNEL, "msg-2", MSG_IDS)).toEqual([]);
  });
});

describe("allActiveUsersRead", () => {
  it("returns true when all active users have read the message", () => {
    applyReadStates(CHANNEL, [
      makeReadState("hash-alice", "Alice", "msg-3"),
      makeReadState("hash-bob", "Bob", "msg-3"),
    ]);
    const result = allActiveUsersRead(
      CHANNEL, "msg-2", MSG_IDS,
      ["hash-alice", "hash-bob", "hash-own"],
      "hash-own",
    );
    expect(result).toBe(true);
  });

  it("returns false when one active user has not read the message", () => {
    applyReadStates(CHANNEL, [
      makeReadState("hash-alice", "Alice", "msg-3"),
      makeReadState("hash-bob", "Bob", "msg-1"),
    ]);
    const result = allActiveUsersRead(
      CHANNEL, "msg-2", MSG_IDS,
      ["hash-alice", "hash-bob", "hash-own"],
      "hash-own",
    );
    expect(result).toBe(false);
  });

  it("returns true when no other active users (only own)", () => {
    const result = allActiveUsersRead(
      CHANNEL, "msg-2", MSG_IDS,
      ["hash-own"],
      "hash-own",
    );
    expect(result).toBe(true);
  });

  it("returns false when activeUserHashes is empty", () => {
    expect(allActiveUsersRead(CHANNEL, "msg-2", MSG_IDS, [], "hash-own")).toBe(false);
  });
});

describe("clearReadReceipts", () => {
  it("removes all stored states", () => {
    applyReadStates(CHANNEL, [makeReadState("hash-alice", "Alice", "msg-2")]);
    expect(getChannelReadStates(CHANNEL)).toHaveLength(1);
    clearReadReceipts();
    expect(getChannelReadStates(CHANNEL)).toEqual([]);
  });
});
