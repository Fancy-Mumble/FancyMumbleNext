/**
 * Unit tests for poll-related logic in ChatView.
 *
 * These test the pure-logic aspects of poll handling:
 *   - Channel-based message filtering (allMessages memo)
 *   - Poll marker regex matching
 *   - Synthetic message dedup
 *   - Target session computation
 *   - Poll payload round-trip (encode → decode → render)
 */

import { describe, it, expect } from "vitest";
import type { PollPayload, PollVotePayload } from "../PollCreator";
import type { ChatMessage } from "../../types";
import { registerPoll, getPoll } from "../PollCard";

// ─── Helpers ──────────────────────────────────────────────────────

/** Replicate the allMessages memo from ChatView. */
function computeAllMessages(
  messages: ChatMessage[],
  pollMessages: ChatMessage[],
  selectedChannel: number | null,
): ChatMessage[] {
  const channelPolls = pollMessages.filter(
    (m) => m.channel_id === selectedChannel,
  );
  return [...messages, ...channelPolls];
}

/** Replicate the poll marker extraction from ChatView. */
function extractPollId(body: string): string | null {
  const match = /<!-- FANCY_POLL:(.+?) -->/.exec(body);
  return match ? match[1] : null;
}

/** Replicate the target computation for poll creation. */
function computePollTargets(
  users: Array<{ session: number; channel_id: number }>,
  selectedChannel: number,
  ownSession: number | null,
): number[] {
  return users
    .filter((u) => u.channel_id === selectedChannel && u.session !== ownSession)
    .map((u) => u.session);
}

/** Replicate the dedup check for setPollMessages. */
function shouldAddPollMessage(
  existing: ChatMessage[],
  pollId: string,
): boolean {
  return !existing.some((m) => m.body.includes(pollId));
}

// ─── Channel filtering ───────────────────────────────────────────

describe("poll channel filtering", () => {
  const pollMsgCh0: ChatMessage = {
    sender_session: 1,
    sender_name: "Alice",
    body: "<!-- FANCY_POLL:poll-ch0 -->",
    channel_id: 0,
    is_own: false,
  };

  const pollMsgCh1: ChatMessage = {
    sender_session: 2,
    sender_name: "Bob",
    body: "<!-- FANCY_POLL:poll-ch1 -->",
    channel_id: 1,
    is_own: false,
  };

  it("shows only polls for the selected channel", () => {
    const result = computeAllMessages([], [pollMsgCh0, pollMsgCh1], 0);
    expect(result).toHaveLength(1);
    expect(result[0].channel_id).toBe(0);
  });

  it("switches visible polls when channel changes", () => {
    const result0 = computeAllMessages([], [pollMsgCh0, pollMsgCh1], 0);
    const result1 = computeAllMessages([], [pollMsgCh0, pollMsgCh1], 1);
    expect(result0).toHaveLength(1);
    expect(result0[0].body).toContain("poll-ch0");
    expect(result1).toHaveLength(1);
    expect(result1[0].body).toContain("poll-ch1");
  });

  it("shows no polls when selectedChannel is null", () => {
    const result = computeAllMessages([], [pollMsgCh0, pollMsgCh1], null);
    expect(result).toHaveLength(0);
  });

  it("appends polls after regular messages", () => {
    const textMsg: ChatMessage = {
      sender_session: 1,
      sender_name: "Alice",
      body: "Hello!",
      channel_id: 0,
      is_own: false,
    };
    const result = computeAllMessages([textMsg], [pollMsgCh0], 0);
    expect(result).toHaveLength(2);
    expect(result[0].body).toBe("Hello!");
    expect(result[1].body).toContain("FANCY_POLL");
  });

  it("shows multiple polls in the same channel", () => {
    const poll2: ChatMessage = {
      sender_session: 3,
      sender_name: "Charlie",
      body: "<!-- FANCY_POLL:poll-ch0-2 -->",
      channel_id: 0,
      is_own: false,
    };
    const result = computeAllMessages([], [pollMsgCh0, poll2], 0);
    expect(result).toHaveLength(2);
  });
});

// ─── Poll marker regex ───────────────────────────────────────────

describe("poll marker extraction", () => {
  it("extracts poll ID from valid marker", () => {
    expect(extractPollId("<!-- FANCY_POLL:abc-123 -->")).toBe("abc-123");
  });

  it("extracts UUID-format poll ID", () => {
    const uuid = "550e8400-e29b-41d4-a716-446655440000";
    expect(extractPollId(`<!-- FANCY_POLL:${uuid} -->`)).toBe(uuid);
  });

  it("returns null for non-poll content", () => {
    expect(extractPollId("Hello world")).toBeNull();
    expect(extractPollId("<b>bold text</b>")).toBeNull();
    expect(extractPollId("<!-- some other comment -->")).toBeNull();
  });

  it("returns null for empty string", () => {
    expect(extractPollId("")).toBeNull();
  });

  it("handles marker with extra HTML around it", () => {
    const body = "text before <!-- FANCY_POLL:test-id --> text after";
    expect(extractPollId(body)).toBe("test-id");
  });
});

// ─── Synthetic message dedup ──────────────────────────────────────

describe("poll message dedup", () => {
  it("allows new poll messages", () => {
    expect(shouldAddPollMessage([], "new-poll")).toBe(true);
  });

  it("prevents duplicate poll messages", () => {
    const existing: ChatMessage[] = [
      {
        sender_session: 1,
        sender_name: "Alice",
        body: "<!-- FANCY_POLL:dup-poll -->",
        channel_id: 0,
        is_own: false,
      },
    ];
    expect(shouldAddPollMessage(existing, "dup-poll")).toBe(false);
  });

  it("allows different poll IDs", () => {
    const existing: ChatMessage[] = [
      {
        sender_session: 1,
        sender_name: "Alice",
        body: "<!-- FANCY_POLL:poll-1 -->",
        channel_id: 0,
        is_own: false,
      },
    ];
    expect(shouldAddPollMessage(existing, "poll-2")).toBe(true);
  });
});

// ─── Target computation ──────────────────────────────────────────

describe("poll target computation", () => {
  const users = [
    { session: 1, channel_id: 0 },
    { session: 2, channel_id: 0 },
    { session: 3, channel_id: 0 },
    { session: 4, channel_id: 1 },
  ];

  it("targets all users in channel except self", () => {
    const targets = computePollTargets(users, 0, 1);
    expect(targets).toEqual([2, 3]);
  });

  it("targets all users in channel when ownSession is null", () => {
    // Bug scenario: ownSession not yet assigned → sends to everyone
    const targets = computePollTargets(users, 0, null);
    expect(targets).toEqual([1, 2, 3]);
  });

  it("returns empty array for channel with only self", () => {
    const targets = computePollTargets(users, 1, 4);
    expect(targets).toEqual([]);
  });

  it("returns empty array for empty user list", () => {
    const targets = computePollTargets([], 0, 1);
    expect(targets).toEqual([]);
  });

  it("handles user in different channel correctly", () => {
    // User 4 is in channel 1 - never included when targeting channel 0
    const targets = computePollTargets(users, 0, 2);
    expect(targets).toEqual([1, 3]);
    expect(targets).not.toContain(4);
  });
});

// ─── Poll payload round-trip ──────────────────────────────────────

describe("poll payload round-trip", () => {
  it("survives JSON encode/decode cycle", () => {
    const poll: PollPayload = {
      type: "poll",
      id: "rt-test",
      question: "Round trip?",
      options: ["Yes", "No"],
      multiple: false,
      creator: 42,
      creatorName: "TestUser",
      createdAt: "2025-01-01T00:00:00Z",
      channelId: 3,
    };

    // Simulate network encoding (TextEncoder → TextDecoder).
    const encoded = new TextEncoder().encode(JSON.stringify(poll));
    const decoded = JSON.parse(new TextDecoder().decode(encoded)) as PollPayload;

    expect(decoded.type).toBe("poll");
    expect(decoded.id).toBe("rt-test");
    expect(decoded.question).toBe("Round trip?");
    expect(decoded.options).toEqual(["Yes", "No"]);
    expect(decoded.channelId).toBe(3);
    expect(decoded.creator).toBe(42);
  });

  it("vote payload survives round-trip", () => {
    const vote: PollVotePayload = {
      type: "poll_vote",
      pollId: "rt-vote",
      selected: [0, 2],
      voter: 10,
      voterName: "Voter",
    };

    const encoded = new TextEncoder().encode(JSON.stringify(vote));
    const decoded = JSON.parse(new TextDecoder().decode(encoded)) as PollVotePayload;

    expect(decoded.type).toBe("poll_vote");
    expect(decoded.pollId).toBe("rt-vote");
    expect(decoded.selected).toEqual([0, 2]);
    expect(decoded.voter).toBe(10);
  });

  it("receiver correctly resolves poll from store after registration", () => {
    const poll: PollPayload = {
      type: "poll",
      id: "store-rt",
      question: "Store test?",
      options: ["A", "B"],
      multiple: false,
      creator: 1,
      creatorName: "Creator",
      createdAt: "2025-01-01T00:00:00Z",
      channelId: 0,
    };

    registerPoll(poll);
    const marker = `<!-- FANCY_POLL:store-rt -->`;
    const pollId = extractPollId(marker);
    expect(pollId).toBe("store-rt");

    const retrieved = getPoll(pollId!);
    expect(retrieved).toBeDefined();
    expect(retrieved?.question).toBe("Store test?");
    expect(retrieved?.channelId).toBe(0);
  });
});

// ─── Bidirectional delivery simulation ────────────────────────────

describe("bidirectional poll delivery", () => {
  it("both users can create and receive polls", () => {
    // Simulate User A creating a poll.
    const pollA: PollPayload = {
      type: "poll",
      id: "bidir-a",
      question: "From A?",
      options: ["Yes", "No"],
      multiple: false,
      creator: 1,
      creatorName: "UserA",
      createdAt: "2025-01-01T00:00:00Z",
      channelId: 0,
    };
    registerPoll(pollA);

    // Simulate User B creating a poll.
    const pollB: PollPayload = {
      type: "poll",
      id: "bidir-b",
      question: "From B?",
      options: ["Yes", "No"],
      multiple: false,
      creator: 2,
      creatorName: "UserB",
      createdAt: "2025-01-01T00:00:01Z",
      channelId: 0,
    };
    registerPoll(pollB);

    // Both polls should be retrievable.
    expect(getPoll("bidir-a")?.creatorName).toBe("UserA");
    expect(getPoll("bidir-b")?.creatorName).toBe("UserB");

    // Simulate message list at User A's end.
    const userAMessages: ChatMessage[] = [
      {
        sender_session: 1,
        sender_name: "UserA",
        body: "<!-- FANCY_POLL:bidir-a -->",
        channel_id: 0,
        is_own: true,
      },
      {
        sender_session: 2,
        sender_name: "UserB",
        body: "<!-- FANCY_POLL:bidir-b -->",
        channel_id: 0,
        is_own: false,
      },
    ];

    // Both should appear when viewing channel 0.
    const visible = computeAllMessages([], userAMessages, 0);
    expect(visible).toHaveLength(2);

    // Both poll IDs should be extractable.
    expect(extractPollId(visible[0].body)).toBe("bidir-a");
    expect(extractPollId(visible[1].body)).toBe("bidir-b");

    // Both should have poll data available.
    expect(getPoll("bidir-a")).toBeDefined();
    expect(getPoll("bidir-b")).toBeDefined();
  });

  it("polls from multiple users in multiple channels", () => {
    const polls: PollPayload[] = [];
    const messages: ChatMessage[] = [];

    // 3 users, 2 channels - each creates a poll.
    for (let i = 0; i < 3; i++) {
      const ch = i < 2 ? 0 : 1;
      const poll: PollPayload = {
        type: "poll",
        id: `multi-${i}`,
        question: `User${i} question?`,
        options: ["X", "Y"],
        multiple: false,
        creator: i + 1,
        creatorName: `User${i}`,
        createdAt: new Date().toISOString(),
        channelId: ch,
      };
      polls.push(poll);
      registerPoll(poll);
      messages.push({
        sender_session: i + 1,
        sender_name: `User${i}`,
        body: `<!-- FANCY_POLL:multi-${i} -->`,
        channel_id: ch,
        is_own: i === 0,
      });
    }

    // Channel 0 should show 2 polls.
    const ch0 = computeAllMessages([], messages, 0);
    expect(ch0).toHaveLength(2);

    // Channel 1 should show 1 poll.
    const ch1 = computeAllMessages([], messages, 1);
    expect(ch1).toHaveLength(1);
    expect(extractPollId(ch1[0].body)).toBe("multi-2");
  });
});
