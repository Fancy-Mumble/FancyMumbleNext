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
  REACTION_DATA_ID,
  CUSTOM_REACTIONS_DATA_ID,
  type ReactionPayload,
} from "../chat/reactionStore";

// -- Helpers -------------------------------------------------------

function makeReaction(overrides: Partial<ReactionPayload> = {}): ReactionPayload {
  return {
    type: "reaction",
    messageId: "msg-1",
    emoji: "\u{1F44D}",
    action: "add",
    reactor: 1,
    reactorName: "Alice",
    channelId: 0,
    ...overrides,
  };
}

// -- Tests ---------------------------------------------------------

beforeEach(() => {
  resetReactions();
});

describe("reactionStore constants", () => {
  it("exports the expected data IDs", () => {
    expect(REACTION_DATA_ID).toBe("fancy-reaction");
    expect(CUSTOM_REACTIONS_DATA_ID).toBe("fancy-custom-reactions");
  });
});

describe("applyReaction + getReactions", () => {
  it("registers a single reaction and returns it", () => {
    applyReaction(makeReaction());
    const reactions = getReactions("msg-1");
    expect(reactions).toHaveLength(1);
    expect(reactions[0].emoji).toBe("\u{1F44D}");
    expect(reactions[0].reactors.size).toBe(1);
    expect(reactions[0].reactors.has(1)).toBe(true);
    expect(reactions[0].reactorNames.get(1)).toBe("Alice");
  });

  it("aggregates multiple users on the same emoji", () => {
    applyReaction(makeReaction({ reactor: 1, reactorName: "Alice" }));
    applyReaction(makeReaction({ reactor: 2, reactorName: "Bob" }));
    applyReaction(makeReaction({ reactor: 3, reactorName: "Charlie" }));

    const reactions = getReactions("msg-1");
    expect(reactions).toHaveLength(1);
    expect(reactions[0].reactors.size).toBe(3);
  });

  it("tracks different emojis separately", () => {
    applyReaction(makeReaction({ emoji: "\u{1F44D}", reactor: 1 }));
    applyReaction(makeReaction({ emoji: "\u{2764}\u{FE0F}", reactor: 2 }));

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
    applyReaction(makeReaction({ reactor: 1 }));
    applyReaction(makeReaction({ reactor: 2, reactorName: "Bob" }));
    applyReaction(makeReaction({ reactor: 1, action: "remove" }));

    const reactions = getReactions("msg-1");
    expect(reactions).toHaveLength(1);
    expect(reactions[0].reactors.has(1)).toBe(false);
    expect(reactions[0].reactors.has(2)).toBe(true);
  });

  it("cleans up empty emoji entries after last reactor removed", () => {
    applyReaction(makeReaction({ reactor: 1 }));
    applyReaction(makeReaction({ reactor: 1, action: "remove" }));

    const reactions = getReactions("msg-1");
    expect(reactions).toHaveLength(0);
  });

  it("does not fail when removing a non-existent reaction", () => {
    // Should not throw.
    applyReaction(makeReaction({ action: "remove" }));
    expect(getReactions("msg-1")).toHaveLength(0);
  });
});

describe("hasReacted", () => {
  it("returns true when the session has reacted", () => {
    applyReaction(makeReaction({ reactor: 42 }));
    expect(hasReacted("msg-1", "\u{1F44D}", 42)).toBe(true);
  });

  it("returns false for a different session", () => {
    applyReaction(makeReaction({ reactor: 42 }));
    expect(hasReacted("msg-1", "\u{1F44D}", 99)).toBe(false);
  });

  it("returns false after removal", () => {
    applyReaction(makeReaction({ reactor: 42 }));
    applyReaction(makeReaction({ reactor: 42, action: "remove" }));
    expect(hasReacted("msg-1", "\u{1F44D}", 42)).toBe(false);
  });

  it("returns false for unknown message", () => {
    expect(hasReacted("nope", "\u{1F44D}", 1)).toBe(false);
  });
});

describe("resetReactions", () => {
  it("clears all reactions", () => {
    applyReaction(makeReaction({ messageId: "a" }));
    applyReaction(makeReaction({ messageId: "b" }));
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
    applyReaction(makeReaction({ messageId: "m1", emoji: "\u{1F44D}", reactor: 1 }));
    applyReaction(makeReaction({ messageId: "m2", emoji: "\u{2764}\u{FE0F}", reactor: 2 }));

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
    applyReaction(makeReaction({ reactor: 1 }));
    applyReaction(makeReaction({ reactor: 1 }));

    const reactions = getReactions("msg-1");
    expect(reactions).toHaveLength(1);
    expect(reactions[0].reactors.size).toBe(1);
  });
});
