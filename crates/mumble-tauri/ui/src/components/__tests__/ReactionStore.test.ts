/**
 * Unit tests for the module-level reaction store.
 *
 * Tests cover: applying reactions, toggling add/remove, querying
 * reaction summaries, and reset behaviour.
 */

import { describe, it, expect, beforeEach } from "vitest";
import {
  applyReaction,
  getReactions,
  hasReacted,
  resetReactions,
  setServerCustomReactions,
  getServerCustomReactions,
} from "../chat/reactionStore";

// -- Tests ---------------------------------------------------------

beforeEach(() => {
  resetReactions();
});

describe("applyReaction + getReactions", () => {
  it("registers a single reaction and returns it", () => {
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-alice", "Alice");
    const reactions = getReactions("msg-1");
    expect(reactions).toHaveLength(1);
    expect(reactions[0].emoji).toBe("\u{1F44D}");
    expect(reactions[0].reactorHashes.size).toBe(1);
    expect(reactions[0].reactorHashes.has("hash-alice")).toBe(true);
    expect(reactions[0].reactorHashNames.get("hash-alice")).toBe("Alice");
  });

  it("aggregates multiple users on the same emoji", () => {
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-alice", "Alice");
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-bob", "Bob");
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-charlie", "Charlie");

    const reactions = getReactions("msg-1");
    expect(reactions).toHaveLength(1);
    expect(reactions[0].reactorHashes.size).toBe(3);
  });

  it("tracks different emojis separately", () => {
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-alice", "Alice");
    applyReaction("msg-1", "\u{2764}\u{FE0F}", "add", "hash-bob", "Bob");

    const reactions = getReactions("msg-1");
    expect(reactions).toHaveLength(2);
    const emojis = reactions.map((r) => r.emoji).sort();
    expect(emojis).toEqual(["\u{1F44D}", "\u{2764}\u{FE0F}"].sort());
  });

  it("returns empty array for unknown message", () => {
    expect(getReactions("nonexistent")).toEqual([]);
  });
});

describe("remove action", () => {
  it("removes a specific reactor", () => {
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-alice", "Alice");
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-bob", "Bob");
    applyReaction("msg-1", "\u{1F44D}", "remove", "hash-alice", "Alice");

    const reactions = getReactions("msg-1");
    expect(reactions).toHaveLength(1);
    expect(reactions[0].reactorHashes.has("hash-alice")).toBe(false);
    expect(reactions[0].reactorHashes.has("hash-bob")).toBe(true);
  });

  it("cleans up empty emoji entries after last reactor removed", () => {
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-alice", "Alice");
    applyReaction("msg-1", "\u{1F44D}", "remove", "hash-alice", "Alice");

    const reactions = getReactions("msg-1");
    expect(reactions).toHaveLength(0);
  });

  it("does not fail when removing a non-existent reaction", () => {
    applyReaction("msg-1", "\u{1F44D}", "remove", "hash-alice", "Alice");
    expect(getReactions("msg-1")).toHaveLength(0);
  });
});

describe("hasReacted", () => {
  it("returns true when the hash has reacted", () => {
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-alice", "Alice");
    expect(hasReacted("msg-1", "\u{1F44D}", "hash-alice")).toBe(true);
  });

  it("returns false for a different hash", () => {
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-alice", "Alice");
    expect(hasReacted("msg-1", "\u{1F44D}", "hash-bob")).toBe(false);
  });

  it("returns false after removal", () => {
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-alice", "Alice");
    applyReaction("msg-1", "\u{1F44D}", "remove", "hash-alice", "Alice");
    expect(hasReacted("msg-1", "\u{1F44D}", "hash-alice")).toBe(false);
  });

  it("returns false for unknown message", () => {
    expect(hasReacted("nope", "\u{1F44D}", "hash-alice")).toBe(false);
  });
});

describe("resetReactions", () => {
  it("clears all reactions", () => {
    applyReaction("a", "\u{1F44D}", "add", "hash-alice", "Alice");
    applyReaction("b", "\u{1F44D}", "add", "hash-bob", "Bob");
    resetReactions();
    expect(getReactions("a")).toHaveLength(0);
    expect(getReactions("b")).toHaveLength(0);
  });

  it("also clears server custom reactions", () => {
    setServerCustomReactions([{ shortcode: ":test:", display: "T" }]);
    resetReactions();
    expect(getServerCustomReactions()).toHaveLength(0);
  });
});

describe("server custom reactions", () => {
  it("stores and retrieves custom reactions", () => {
    const customs = [
      { shortcode: ":mumble:", display: "\u{1F3A4}", label: "Mumble" },
      { shortcode: ":wave:", display: "\u{1F44B}" },
    ];
    setServerCustomReactions(customs);
    expect(getServerCustomReactions()).toEqual(customs);
  });

  it("overwrites previous custom reactions", () => {
    setServerCustomReactions([{ shortcode: ":a:", display: "A" }]);
    setServerCustomReactions([{ shortcode: ":b:", display: "B" }]);
    expect(getServerCustomReactions()).toHaveLength(1);
    expect(getServerCustomReactions()[0].shortcode).toBe(":b:");
  });
});

describe("multi-message isolation", () => {
  it("reactions on different messages do not interfere", () => {
    applyReaction("m1", "\u{1F44D}", "add", "hash-alice", "Alice");
    applyReaction("m2", "\u{2764}\u{FE0F}", "add", "hash-bob", "Bob");

    const r1 = getReactions("m1");
    const r2 = getReactions("m2");
    expect(r1).toHaveLength(1);
    expect(r1[0].emoji).toBe("\u{1F44D}");
    expect(r2).toHaveLength(1);
    expect(r2[0].emoji).toBe("\u{2764}\u{FE0F}");
  });
});

describe("idempotence", () => {
  it("adding the same reaction twice does not duplicate", () => {
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-alice", "Alice");
    applyReaction("msg-1", "\u{1F44D}", "add", "hash-alice", "Alice");

    const reactions = getReactions("msg-1");
    expect(reactions).toHaveLength(1);
    expect(reactions[0].reactorHashes.size).toBe(1);
  });
});

describe("cross-user message_id consistency (regression)", () => {
  it("reactions keyed by the SAME message_id are visible to all users", () => {
    // Both users must share the same message_id for a given message.
    // The server preserves the sender's client-generated UUID so all
    // participants reference the same ID.
    const sharedMessageId = "shared-uuid-123";

    applyReaction(sharedMessageId, "\u{1F44D}", "add", "hash-alice", "Alice");
    applyReaction(sharedMessageId, "\u{1F44D}", "add", "hash-bob", "Bob");

    const reactions = getReactions(sharedMessageId);
    expect(reactions).toHaveLength(1);
    expect(reactions[0].reactorHashes.size).toBe(2);
    expect(reactions[0].reactorHashes.has("hash-alice")).toBe(true);
    expect(reactions[0].reactorHashes.has("hash-bob")).toBe(true);
  });

  it("reactions with DIFFERENT message_ids do NOT collide", () => {
    // If the server were to overwrite the client message_id, each user
    // would end up with a different UUID for the same message. Reactions
    // would then be isolated per-user and invisible to others.
    applyReaction("uuid-from-sender", "\u{1F44D}", "add", "hash-alice", "Alice");
    applyReaction("uuid-from-server", "\u{1F44D}", "add", "hash-bob", "Bob");

    const fromSender = getReactions("uuid-from-sender");
    const fromServer = getReactions("uuid-from-server");

    // Each ID sees only one reactor - the reaction worlds are split.
    expect(fromSender).toHaveLength(1);
    expect(fromSender[0].reactorHashes.size).toBe(1);
    expect(fromServer).toHaveLength(1);
    expect(fromServer[0].reactorHashes.size).toBe(1);
  });

  it("hasReacted correctly identifies own reaction via cert hash", () => {
    const messageId = "consistent-uuid";
    applyReaction(messageId, "\u{1F44D}", "add", "hash-me", "Me");
    applyReaction(messageId, "\u{1F44D}", "add", "hash-other", "Other");

    expect(hasReacted(messageId, "\u{1F44D}", "hash-me")).toBe(true);
    expect(hasReacted(messageId, "\u{1F44D}", "hash-other")).toBe(true);
    expect(hasReacted(messageId, "\u{1F44D}", "hash-nobody")).toBe(false);
  });

  it("remove action by one user does not affect others on the same message", () => {
    const messageId = "shared-uuid";
    applyReaction(messageId, "\u{1F44D}", "add", "hash-alice", "Alice");
    applyReaction(messageId, "\u{1F44D}", "add", "hash-bob", "Bob");
    applyReaction(messageId, "\u{1F44D}", "remove", "hash-alice", "Alice");

    expect(hasReacted(messageId, "\u{1F44D}", "hash-alice")).toBe(false);
    expect(hasReacted(messageId, "\u{1F44D}", "hash-bob")).toBe(true);

    const reactions = getReactions(messageId);
    expect(reactions).toHaveLength(1);
    expect(reactions[0].reactorHashes.size).toBe(1);
  });
});
