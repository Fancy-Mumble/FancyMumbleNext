/**
 * Unit tests for PollCard React component rendering.
 *
 * Uses @testing-library/react to verify the component renders
 * polls correctly, handles voting, and displays proper states.
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import PollCard, { registerVote, registerLocalVote } from "../chat/PollCard";
import type { PollPayload } from "../chat/PollCreator";

// --- Helpers ------------------------------------------------------

function makePoll(overrides: Partial<PollPayload> = {}): PollPayload {
  return {
    type: "poll",
    id: `render-${Math.random().toString(36).slice(2)}`,
    question: "What's your favourite colour?",
    options: ["Red", "Blue", "Green"],
    multiple: false,
    creator: 1,
    creatorName: "Alice",
    createdAt: "2025-01-01T00:00:00Z",
    channelId: 0,
    ...overrides,
  };
}

// --- Rendering tests ---------------------------------------------

describe("PollCard rendering", () => {
  it("displays the poll question", () => {
    const poll = makePoll({ question: "Favourite framework?" });
    render(<PollCard poll={poll} ownSession={10} onVote={vi.fn()} />);
    expect(screen.getByText("Favourite framework?")).toBeTruthy();
  });

  it("displays all options", () => {
    const poll = makePoll({ options: ["React", "Vue", "Svelte"] });
    render(<PollCard poll={poll} ownSession={10} onVote={vi.fn()} />);
    expect(screen.getByText("React")).toBeTruthy();
    expect(screen.getByText("Vue")).toBeTruthy();
    expect(screen.getByText("Svelte")).toBeTruthy();
  });

  it("shows creator name", () => {
    const poll = makePoll({ creatorName: "BobTheCreator" });
    render(<PollCard poll={poll} ownSession={10} onVote={vi.fn()} />);
    expect(screen.getByText("by BobTheCreator")).toBeTruthy();
  });

  it("shows 0 votes initially", () => {
    const poll = makePoll();
    render(<PollCard poll={poll} ownSession={10} onVote={vi.fn()} />);
    expect(screen.getByText("0 votes")).toBeTruthy();
  });

  it("renders as a poll (has Poll label)", () => {
    const poll = makePoll();
    render(<PollCard poll={poll} ownSession={10} onVote={vi.fn()} />);
    expect(screen.getByText("Poll")).toBeTruthy();
  });
});

// --- Voting tests -------------------------------------------------

describe("PollCard voting", () => {
  it("calls onVote when single-choice option is clicked", () => {
    const onVote = vi.fn();
    const poll = makePoll({ id: "vote-test-1", options: ["A", "B"] });
    render(<PollCard poll={poll} ownSession={10} onVote={onVote} />);

    fireEvent.click(screen.getByText("A"));
    expect(onVote).toHaveBeenCalledWith("vote-test-1", [0]);
  });

  it("does not call onVote twice after voting (single choice)", () => {
    const onVote = vi.fn();
    const poll = makePoll({ id: "vote-test-2", options: ["A", "B"] });

    // Pre-register a local vote so the component thinks we've voted.
    registerLocalVote("vote-test-2", [0]);

    render(<PollCard poll={poll} ownSession={10} onVote={onVote} />);

    // Options should be disabled after voting.
    fireEvent.click(screen.getByText("A"));
    expect(onVote).not.toHaveBeenCalled();
  });

  it("shows radio indicators for single-choice polls", () => {
    const poll = makePoll({ multiple: false, id: "radio-test" });
    render(<PollCard poll={poll} ownSession={10} onVote={vi.fn()} />);
    // Single-choice shows radio markers (O)
    expect(screen.getAllByText("○")).toHaveLength(3);
  });

  it("shows checkbox indicators for multiple-choice polls", () => {
    const poll = makePoll({ multiple: true, id: "check-test" });
    render(<PollCard poll={poll} ownSession={10} onVote={vi.fn()} />);
    // Multiple choice shows checkboxes ([_])
    expect(screen.getAllByText("☐")).toHaveLength(3);
  });
});

// --- Own vs other rendering ---------------------------------------

describe("PollCard own vs other", () => {
  it("renders with isOwn=true", () => {
    const poll = makePoll({ question: "My own poll?" });
    const { container } = render(
      <PollCard poll={poll} ownSession={1} isOwn={true} onVote={vi.fn()} />,
    );
    // Should have the own-style card class
    expect(container.firstChild).toBeTruthy();
  });

  it("renders with isOwn=false", () => {
    const poll = makePoll({ question: "Someone else's poll?" });
    const { container } = render(
      <PollCard poll={poll} ownSession={2} isOwn={false} onVote={vi.fn()} />,
    );
    expect(container.firstChild).toBeTruthy();
  });

  it("renders with null ownSession (not yet assigned)", () => {
    const poll = makePoll({ question: "No session yet?" });
    render(
      <PollCard poll={poll} ownSession={null} isOwn={false} onVote={vi.fn()} />,
    );
    expect(screen.getByText("No session yet?")).toBeTruthy();
    // User should still be able to vote even without ownSession
    expect(screen.getByText("0 votes")).toBeTruthy();
  });
});

// --- Vote display -------------------------------------------------

describe("PollCard vote display", () => {
  it("shows percentages after voting", () => {
    const pollId = "pct-test";
    const poll = makePoll({ id: pollId, options: ["A", "B"] });

    // Register some votes.
    registerVote({ type: "poll_vote", pollId, selected: [0], voter: 1, voterName: "V1" });
    registerVote({ type: "poll_vote", pollId, selected: [0], voter: 2, voterName: "V2" });
    registerVote({ type: "poll_vote", pollId, selected: [1], voter: 3, voterName: "V3" });

    // Mark local user as voted.
    registerLocalVote(pollId, [0]);

    render(<PollCard poll={poll} ownSession={1} onVote={vi.fn()} />);

    // Should show percentages (67% for A, 33% for B).
    expect(screen.getByText("67%")).toBeTruthy();
    expect(screen.getByText("33%")).toBeTruthy();
  });

  it("shows vote count", () => {
    const pollId = "count-test";
    const poll = makePoll({ id: pollId, options: ["X", "Y"] });
    registerVote({ type: "poll_vote", pollId, selected: [0], voter: 10, voterName: "U1" });
    registerVote({ type: "poll_vote", pollId, selected: [1], voter: 11, voterName: "U2" });
    registerLocalVote(pollId, [0]);

    render(<PollCard poll={poll} ownSession={10} onVote={vi.fn()} />);
    expect(screen.getByText("2 votes")).toBeTruthy();
  });
});
