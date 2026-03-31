/**
 * Unit tests for ReactionBar React component rendering.
 *
 * Uses @testing-library/react to verify pills render correctly,
 * active state highlights the user's own reactions, the "+" button
 * is present, and click callbacks are invoked properly.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ReactionBar from "../chat/ReactionBar";
import type { ReactionSummary } from "../chat/reactionStore";

// Mock platform module so isMobile is false (desktop tooltips enabled).
vi.mock("../../utils/platform", () => ({ isMobile: false }));

// -- Helpers -------------------------------------------------------

function makeSummary(emoji: string, reactors: [number, string][], firstTimestamp = 0): ReactionSummary {
  return {
    emoji,
    reactors: new Set(reactors.map(([id]) => id)),
    reactorNames: new Map(reactors),
    reactorHashes: new Set(),
    reactorHashNames: new Map(),
    firstTimestamp,
  };
}

// -- Tests ---------------------------------------------------------

describe("ReactionBar rendering", () => {
  const onToggle = vi.fn();
  const onAdd = vi.fn();

  beforeEach(() => {
    onToggle.mockClear();
    onAdd.mockClear();
  });

  it("renders nothing when reactions are empty", () => {
    const { container } = render(
      <ReactionBar reactions={[]} ownSession={1} onToggle={onToggle} onAdd={onAdd} />,
    );
    expect(container.innerHTML).toBe("");
  });

  it("renders a pill for each reaction emoji", () => {
    const reactions = [
      makeSummary("\u{1F44D}", [[1, "Alice"]]),
      makeSummary("\u{2764}\u{FE0F}", [[2, "Bob"]]),
    ];
    render(<ReactionBar reactions={reactions} ownSession={99} onToggle={onToggle} onAdd={onAdd} />);

    expect(screen.getByText("\u{1F44D}")).toBeTruthy();
    expect(screen.getByText("\u{2764}\u{FE0F}")).toBeTruthy();
  });

  it("shows the reaction count", () => {
    const reactions = [
      makeSummary("\u{1F44D}", [[1, "Alice"], [2, "Bob"], [3, "Charlie"]]),
    ];
    render(<ReactionBar reactions={reactions} ownSession={99} onToggle={onToggle} onAdd={onAdd} />);

    expect(screen.getByText("3")).toBeTruthy();
  });

  it("marks the pill active when own session has reacted", () => {
    const reactions = [makeSummary("\u{1F44D}", [[42, "Me"]])];
    render(<ReactionBar reactions={reactions} ownSession={42} onToggle={onToggle} onAdd={onAdd} />);

    const btn = screen.getByLabelText("\u{1F44D} 1");
    expect(btn.className).toContain("Active");
  });

  it("does NOT mark pill active when own session has NOT reacted", () => {
    const reactions = [makeSummary("\u{1F44D}", [[1, "Alice"]])];
    render(<ReactionBar reactions={reactions} ownSession={99} onToggle={onToggle} onAdd={onAdd} />);

    const btn = screen.getByLabelText("\u{1F44D} 1");
    expect(btn.className).not.toContain("Active");
  });

  it("calls onToggle with the emoji when a pill is clicked", () => {
    const reactions = [makeSummary("\u{1F44D}", [[1, "Alice"]])];
    render(<ReactionBar reactions={reactions} ownSession={1} onToggle={onToggle} onAdd={onAdd} />);

    fireEvent.click(screen.getByLabelText("\u{1F44D} 1"));
    expect(onToggle).toHaveBeenCalledWith("\u{1F44D}");
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("renders the add button and calls onAdd when clicked", () => {
    const reactions = [makeSummary("\u{1F44D}", [[1, "Alice"]])];
    render(<ReactionBar reactions={reactions} ownSession={1} onToggle={onToggle} onAdd={onAdd} />);

    const addBtn = screen.getByLabelText("Add reaction");
    expect(addBtn).toBeTruthy();
    fireEvent.click(addBtn);
    expect(onAdd).toHaveBeenCalledTimes(1);
  });

  it("preserves insertion order (sorted by firstTimestamp from store)", () => {
    // Reactions come pre-sorted by firstTimestamp from getReactions().
    const reactions = [
      makeSummary("\u{1F44D}", [[1, "A"], [2, "B"], [3, "C"]], 100),
      makeSummary("\u{2764}\u{FE0F}", [[1, "A"], [2, "B"]], 200),
      makeSummary("\u{1F525}", [[1, "A"]], 300),
    ];
    render(<ReactionBar reactions={reactions} ownSession={99} onToggle={onToggle} onAdd={onAdd} />);

    // Buttons should preserve the order given (by firstTimestamp), then add.
    const buttons = screen.getAllByRole("button");
    expect(buttons[0].getAttribute("aria-label")).toBe("\u{1F44D} 3");
    expect(buttons[1].getAttribute("aria-label")).toBe("\u{2764}\u{FE0F} 2");
    expect(buttons[2].getAttribute("aria-label")).toBe("\u{1F525} 1");
    expect(buttons[3].getAttribute("aria-label")).toBe("Add reaction");
  });

  it("marks pill active when own hash matches a reactor hash", () => {
    const summary: ReactionSummary = {
      emoji: "\u{1F44D}",
      reactors: new Set(),
      reactorNames: new Map(),
      reactorHashes: new Set(["abc123"]),
      reactorHashNames: new Map([["abc123", "Me"]]),
      firstTimestamp: 0,
    };
    render(
      <ReactionBar
        reactions={[summary]}
        ownSession={99}
        ownHash="abc123"
        onToggle={onToggle}
        onAdd={onAdd}
      />,
    );
    const btn = screen.getByLabelText("\u{1F44D} 1");
    expect(btn.className).toContain("Active");
  });

  it("counts both session reactors and hash reactors", () => {
    const summary: ReactionSummary = {
      emoji: "\u{1F44D}",
      reactors: new Set([1, 2]),
      reactorNames: new Map([[1, "Alice"], [2, "Bob"]]),
      reactorHashes: new Set(["hash1"]),
      reactorHashNames: new Map([["hash1", "Charlie"]]),
      firstTimestamp: 0,
    };
    render(
      <ReactionBar reactions={[summary]} ownSession={99} onToggle={onToggle} onAdd={onAdd} />,
    );
    // 2 session + 1 hash = 3
    expect(screen.getByText("3")).toBeTruthy();
  });
});
