/**
 * Unit tests for PollCard module-level stores and helpers.
 *
 * These test the poll registration, vote tracking, and local vote
 * bookkeeping that PollCard.tsx exports at module scope.
 */

import { describe, it, expect } from "vitest";
import {
  registerPoll,
  getPoll,
  registerVote,
  getVotes,
  registerLocalVote,
  getLocalVote,
} from "../chat/PollCard";
import type { PollPayload, PollVotePayload } from "../chat/PollCreator";

// --- Helpers ------------------------------------------------------

function makePoll(overrides: Partial<PollPayload> = {}): PollPayload {
  return {
    type: "poll",
    id: `poll-${Math.random().toString(36).slice(2)}`,
    question: "Test question?",
    options: ["A", "B", "C"],
    multiple: false,
    creator: 1,
    creatorName: "Alice",
    createdAt: new Date().toISOString(),
    channelId: 0,
    ...overrides,
  };
}

function makeVote(overrides: Partial<PollVotePayload> = {}): PollVotePayload {
  return {
    type: "poll_vote",
    pollId: "test-poll",
    selected: [0],
    voter: 2,
    voterName: "Bob",
    ...overrides,
  };
}

// --- Poll store ---------------------------------------------------

describe("pollStore", () => {
  it("registers and retrieves a poll", () => {
    const poll = makePoll({ id: "ps-1" });
    registerPoll(poll);
    expect(getPoll("ps-1")).toBe(poll);
  });

  it("returns undefined for unknown poll ID", () => {
    expect(getPoll("nonexistent")).toBeUndefined();
  });

  it("overwrites a poll with the same ID", () => {
    const poll1 = makePoll({ id: "ps-ow", question: "First?" });
    const poll2 = makePoll({ id: "ps-ow", question: "Second?" });
    registerPoll(poll1);
    registerPoll(poll2);
    expect(getPoll("ps-ow")?.question).toBe("Second?");
  });

  it("stores polls for different channels independently", () => {
    const pollCh0 = makePoll({ id: "ch0-poll", channelId: 0 });
    const pollCh1 = makePoll({ id: "ch1-poll", channelId: 1 });
    registerPoll(pollCh0);
    registerPoll(pollCh1);
    expect(getPoll("ch0-poll")?.channelId).toBe(0);
    expect(getPoll("ch1-poll")?.channelId).toBe(1);
  });
});

// --- Vote store ---------------------------------------------------

describe("voteStore", () => {
  it("registers and retrieves a vote", () => {
    const vote = makeVote({ pollId: "vs-1", voter: 10 });
    registerVote(vote);
    const votes = getVotes("vs-1");
    expect(votes).toHaveLength(1);
    expect(votes[0].voter).toBe(10);
  });

  it("returns empty array for poll with no votes", () => {
    expect(getVotes("no-votes")).toEqual([]);
  });

  it("replaces a previous vote by the same voter", () => {
    registerVote(makeVote({ pollId: "vs-dup", voter: 5, selected: [0] }));
    registerVote(makeVote({ pollId: "vs-dup", voter: 5, selected: [1] }));
    const votes = getVotes("vs-dup");
    expect(votes).toHaveLength(1);
    expect(votes[0].selected).toEqual([1]);
  });

  it("accumulates votes from different voters", () => {
    registerVote(makeVote({ pollId: "vs-multi", voter: 1 }));
    registerVote(makeVote({ pollId: "vs-multi", voter: 2 }));
    registerVote(makeVote({ pollId: "vs-multi", voter: 3 }));
    expect(getVotes("vs-multi")).toHaveLength(3);
  });

  it("keeps votes isolated between polls", () => {
    registerVote(makeVote({ pollId: "iso-a", voter: 1 }));
    registerVote(makeVote({ pollId: "iso-b", voter: 2 }));
    expect(getVotes("iso-a")).toHaveLength(1);
    expect(getVotes("iso-b")).toHaveLength(1);
  });
});

// --- Local vote tracking ------------------------------------------

describe("localVotes", () => {
  it("records and retrieves a local vote", () => {
    registerLocalVote("lv-1", [2]);
    expect(getLocalVote("lv-1")).toEqual([2]);
  });

  it("returns undefined for unvoted poll", () => {
    expect(getLocalVote("unvoted")).toBeUndefined();
  });

  it("overwrites previous local vote", () => {
    registerLocalVote("lv-ow", [0]);
    registerLocalVote("lv-ow", [1, 2]);
    expect(getLocalVote("lv-ow")).toEqual([1, 2]);
  });
});

// --- Poll payload structure ---------------------------------------

describe("PollPayload", () => {
  it("includes channelId field for channel-aware delivery", () => {
    const poll = makePoll({ channelId: 5 });
    expect(poll.channelId).toBe(5);
  });

  it("serializes to JSON matching the wire format", () => {
    const poll = makePoll({
      id: "wire-test",
      question: "Best lang?",
      options: ["Rust", "TS"],
      multiple: false,
      creator: 42,
      creatorName: "Dev",
      channelId: 0,
    });
    const json = JSON.parse(JSON.stringify(poll));
    expect(json.type).toBe("poll");
    expect(json.channelId).toBe(0);
    expect(json.creator).toBe(42);
    expect(json.options).toEqual(["Rust", "TS"]);
  });

  it("channelId defaults to 0 via nullish coalescing", () => {
    // Simulate receiving a poll without channelId (backward compat).
    const payload = JSON.parse(
      '{"type":"poll","id":"old","question":"Q?","options":["A","B"],"multiple":false,"creator":1,"creatorName":"X","createdAt":"2025-01-01T00:00:00Z"}'
    );
    const channelId: number = payload.channelId ?? 0;
    expect(channelId).toBe(0);
  });
});
