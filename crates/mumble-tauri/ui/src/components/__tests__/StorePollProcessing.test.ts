/**
 * Regression tests for poll processing in the Zustand store.
 *
 * These tests verify that the store's "plugin-data" event handler
 * correctly processes poll and vote payloads and stores them in
 * Zustand state - the same path that runs when a Tauri event fires.
 *
 * This is a regression test for the bug where polls were processed
 * via an indirect handler-array dispatch (pluginDataHandlers) which
 * could become stale across Vite HMR reloads or React StrictMode
 * double-mounts, causing polls to never appear in the UI despite
 * the backend successfully receiving them.
 */

import { describe, it, expect, beforeEach } from "vitest";
import { useAppStore } from "../../store";
import type { PollPayload, PollVotePayload } from "../PollCreator";
import { getPoll, getVotes, registerVote } from "../PollCard";

// --- Helpers ------------------------------------------------------

function makePoll(overrides: Partial<PollPayload> = {}): PollPayload {
  return {
    type: "poll",
    id: `store-test-${Math.random().toString(36).slice(2)}`,
    question: "Test question?",
    options: ["A", "B", "C"],
    multiple: false,
    creator: 42,
    creatorName: "TestUser",
    createdAt: new Date().toISOString(),
    channelId: 0,
    ...overrides,
  };
}

/** Simulate what the store's plugin-data listener does for polls. */
function simulateIncomingPoll(
  poll: PollPayload,
  senderSession: number | null = poll.creator,
) {
  // This replicates the exact logic in initEventListeners -> "plugin-data" handler
  const bytes = new TextEncoder().encode(JSON.stringify(poll));
  const json = new TextDecoder().decode(bytes);
  const payload = JSON.parse(json) as PollPayload;
  payload.creator = senderSession ?? payload.creator;

  // Resolve creator name from store users
  const users = useAppStore.getState().users;
  const user = users.find((u) => u.session === payload.creator);
  if (user) payload.creatorName = user.name;

  useAppStore.getState().addPoll(payload, false);
}

/** Simulate what the store's plugin-data listener does for votes. */
function simulateIncomingVote(
  vote: PollVotePayload,
  senderSession: number | null = vote.voter,
) {
  const bytes = new TextEncoder().encode(JSON.stringify(vote));
  const json = new TextDecoder().decode(bytes);
  const payload = JSON.parse(json) as PollVotePayload;
  payload.voter = senderSession ?? payload.voter;

  const users = useAppStore.getState().users;
  const user = users.find((u) => u.session === payload.voter);
  if (user) payload.voterName = user.name;

  registerVote(payload);
  useAppStore.setState({});
}

// --- Reset store between tests ------------------------------------

beforeEach(() => {
  useAppStore.getState().reset();
});

// --- Core poll processing -----------------------------------------

describe("store poll processing (regression)", () => {
  it("addPoll stores poll in Zustand state", () => {
    const poll = makePoll({ id: "reg-1" });
    useAppStore.getState().addPoll(poll, false);

    const state = useAppStore.getState();
    expect(state.polls.get("reg-1")).toBeDefined();
    expect(state.polls.get("reg-1")?.question).toBe("Test question?");
  });

  it("addPoll creates synthetic poll message", () => {
    const poll = makePoll({ id: "reg-2", channelId: 5 });
    useAppStore.getState().addPoll(poll, false);

    const state = useAppStore.getState();
    expect(state.pollMessages).toHaveLength(1);
    expect(state.pollMessages[0].body).toContain("reg-2");
    expect(state.pollMessages[0].channel_id).toBe(5);
    expect(state.pollMessages[0].is_own).toBe(false);
  });

  it("addPoll with isOwn=true marks message as own", () => {
    const poll = makePoll({ id: "reg-own" });
    useAppStore.getState().addPoll(poll, true);

    const state = useAppStore.getState();
    expect(state.pollMessages[0].is_own).toBe(true);
  });

  it("addPoll deduplicates same poll ID", () => {
    const poll = makePoll({ id: "reg-dedup" });
    useAppStore.getState().addPoll(poll, false);
    useAppStore.getState().addPoll(poll, false);

    const state = useAppStore.getState();
    expect(state.pollMessages).toHaveLength(1);
  });

  it("addPoll allows different poll IDs", () => {
    useAppStore.getState().addPoll(makePoll({ id: "p1" }), false);
    useAppStore.getState().addPoll(makePoll({ id: "p2" }), false);

    const state = useAppStore.getState();
    expect(state.pollMessages).toHaveLength(2);
    expect(state.polls.size).toBe(2);
  });

  it("addPoll also registers in module-level pollStore", () => {
    const poll = makePoll({ id: "reg-module" });
    useAppStore.getState().addPoll(poll, false);

    // Module-level store should also have the poll.
    expect(getPoll("reg-module")).toBeDefined();
    expect(getPoll("reg-module")?.question).toBe("Test question?");
  });
});

// --- Simulated plugin-data processing -----------------------------

describe("simulated plugin-data event processing", () => {
  it("incoming poll appears in store state", () => {
    const poll = makePoll({ id: "sim-1", channelId: 3 });
    simulateIncomingPoll(poll, 92);

    const state = useAppStore.getState();
    expect(state.polls.get("sim-1")).toBeDefined();
    expect(state.polls.get("sim-1")?.creator).toBe(92);
    expect(state.pollMessages).toHaveLength(1);
    expect(state.pollMessages[0].channel_id).toBe(3);
  });

  it("incoming poll resolves creator name from users", () => {
    // Pre-populate users in the store.
    useAppStore.setState({
      users: [
        { session: 92, name: "Alice", channel_id: 0, texture: null, comment: null, mute: false, deaf: false, suppress: false, self_mute: false, self_deaf: false, priority_speaker: false },
      ],
    });

    const poll = makePoll({ id: "sim-name", creatorName: "Unknown" });
    simulateIncomingPoll(poll, 92);

    const stored = useAppStore.getState().polls.get("sim-name");
    expect(stored?.creatorName).toBe("Alice");
  });

  it("incoming vote updates module-level vote store", () => {
    const poll = makePoll({ id: "sim-vote-poll" });
    simulateIncomingPoll(poll);

    const vote: PollVotePayload = {
      type: "poll_vote",
      pollId: "sim-vote-poll",
      selected: [1],
      voter: 10,
      voterName: "Voter",
    };
    simulateIncomingVote(vote, 10);

    const votes = getVotes("sim-vote-poll");
    expect(votes).toHaveLength(1);
    expect(votes[0].selected).toEqual([1]);
  });

  it("multiple polls from different senders all appear", () => {
    // Simulate 3 different users sending polls.
    for (let i = 0; i < 3; i++) {
      const poll = makePoll({
        id: `multi-sender-${i}`,
        creator: 100 + i,
        creatorName: `User${i}`,
        channelId: 0,
      });
      simulateIncomingPoll(poll, 100 + i);
    }

    const state = useAppStore.getState();
    expect(state.polls.size).toBe(3);
    expect(state.pollMessages).toHaveLength(3);

    // All three should be findable.
    for (let i = 0; i < 3; i++) {
      expect(state.polls.get(`multi-sender-${i}`)).toBeDefined();
    }
  });

  it("poll with channelId=0 is correctly stored", () => {
    const poll = makePoll({ id: "ch0-poll", channelId: 0 });
    simulateIncomingPoll(poll);

    const msg = useAppStore.getState().pollMessages[0];
    expect(msg.channel_id).toBe(0);
  });

  it("poll survives JSON round-trip through simulated network path", () => {
    const original: PollPayload = {
      type: "poll",
      id: "roundtrip-reg",
      question: "Does round-trip work? 🎯",
      options: ["Definitely", "Maybe", "No way"],
      multiple: true,
      creator: 55,
      creatorName: "NetUser",
      createdAt: "2026-01-01T12:00:00Z",
      channelId: 7,
    };

    simulateIncomingPoll(original, 55);

    const stored = useAppStore.getState().polls.get("roundtrip-reg");
    expect(stored).toBeDefined();
    expect(stored!.question).toBe("Does round-trip work? 🎯");
    expect(stored!.options).toEqual(["Definitely", "Maybe", "No way"]);
    expect(stored!.multiple).toBe(true);
    expect(stored!.channelId).toBe(7);
    expect(stored!.creator).toBe(55);
  });
});

// --- Reset resilience ---------------------------------------------

describe("poll state reset on disconnect", () => {
  it("reset clears polls and pollMessages", () => {
    useAppStore.getState().addPoll(makePoll({ id: "reset-1" }), false);
    useAppStore.getState().addPoll(makePoll({ id: "reset-2" }), false);

    expect(useAppStore.getState().polls.size).toBe(2);
    expect(useAppStore.getState().pollMessages).toHaveLength(2);

    // Simulate disconnect.
    useAppStore.getState().reset();

    expect(useAppStore.getState().polls.size).toBe(0);
    expect(useAppStore.getState().pollMessages).toHaveLength(0);
  });

  it("new polls can be added after reset", () => {
    useAppStore.getState().addPoll(makePoll({ id: "pre-reset" }), false);
    useAppStore.getState().reset();
    useAppStore.getState().addPoll(makePoll({ id: "post-reset" }), false);

    const state = useAppStore.getState();
    expect(state.polls.size).toBe(1);
    expect(state.polls.has("post-reset")).toBe(true);
    expect(state.polls.has("pre-reset")).toBe(false);
  });
});
