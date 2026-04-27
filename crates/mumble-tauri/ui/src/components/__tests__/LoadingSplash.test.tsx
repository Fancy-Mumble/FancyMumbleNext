/**
 * Unit tests for the LoadingSplash component.
 *
 * Verifies that the rotating subtitle cycles, that the override
 * `message` prop pins the text, and that the message pool is sane.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, act } from "@testing-library/react";
import LoadingSplash, {
  __TEST_FUNNY_MESSAGES,
} from "../elements/LoadingSplash";

describe("LoadingSplash", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders the default title and a message from the pool", () => {
    render(<LoadingSplash />);
    expect(screen.getByText("Fancy Mumble")).toBeTruthy();
    const status = screen.getByRole("status");
    const subtitle = status.lastElementChild?.textContent ?? "";
    expect(__TEST_FUNNY_MESSAGES).toContain(subtitle);
  });

  it("rotates the subtitle on a timer", () => {
    render(<LoadingSplash />);
    const status = screen.getByRole("status");
    const initial = status.lastElementChild?.textContent ?? "";

    // Advance enough for at least a couple of rotations.
    act(() => {
      vi.advanceTimersByTime(1800 * 5);
    });
    const later = status.lastElementChild?.textContent ?? "";
    expect(__TEST_FUNNY_MESSAGES).toContain(later);
    // The pool has 17+ entries, so 5 rotations should reach a different one.
    expect(later).not.toBe(initial);
  });

  it("respects an explicit message and never rotates it", () => {
    render(<LoadingSplash message="Pinned text" />);
    expect(screen.getByText("Pinned text")).toBeTruthy();
    act(() => {
      vi.advanceTimersByTime(1800 * 10);
    });
    expect(screen.getByText("Pinned text")).toBeTruthy();
  });

  it("ships a non-empty pool of unique messages", () => {
    expect(__TEST_FUNNY_MESSAGES.length).toBeGreaterThanOrEqual(8);
    const unique = new Set(__TEST_FUNNY_MESSAGES);
    expect(unique.size).toBe(__TEST_FUNNY_MESSAGES.length);
  });
});
