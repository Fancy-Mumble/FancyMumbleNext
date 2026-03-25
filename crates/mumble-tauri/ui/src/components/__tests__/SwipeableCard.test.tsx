/**
 * Unit tests for SwipeableCard component.
 *
 * Verifies swipe gesture handling: threshold detection, action triggering,
 * direction clamping, and disabled state.
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import SwipeableCard from "../elements/SwipeableCard";

// --- Helpers -------------------------------------------------------

/** Simulate a touch swipe on an element. */
function simulateSwipe(
  el: HTMLElement,
  deltaX: number,
  deltaY = 0,
) {
  const startX = 200;
  const startY = 200;

  fireEvent.touchStart(el, {
    touches: [{ clientX: startX, clientY: startY }],
  });
  fireEvent.touchMove(el, {
    touches: [{ clientX: startX + deltaX, clientY: startY + deltaY }],
  });
  fireEvent.touchEnd(el, {
    changedTouches: [{ clientX: startX + deltaX, clientY: startY + deltaY }],
  });
}

// --- Tests ---------------------------------------------------------

describe("SwipeableCard", () => {
  it("renders children", () => {
    render(
      <SwipeableCard>
        <span>Hello Card</span>
      </SwipeableCard>,
    );
    expect(screen.getByText("Hello Card")).toBeTruthy();
  });

  it("triggers left swipe action when threshold exceeded", async () => {
    const onDelete = vi.fn();
    const { container } = render(
      <SwipeableCard
        leftSwipeAction={{
          label: "Delete",
          icon: "\u2715",
          color: "red",
          onTrigger: onDelete,
        }}
        threshold={60}
      >
        <span>Server A</span>
      </SwipeableCard>,
    );

    // The swipeable div is the .content child
    const content = container.querySelector("[class*='content']")!;
    simulateSwipe(content as HTMLElement, -100);

    // onTrigger is called after a short settle timeout
    await vi.waitFor(() => expect(onDelete).toHaveBeenCalledTimes(1));
  });

  it("triggers right swipe action when threshold exceeded", async () => {
    const onFavorite = vi.fn();
    const { container } = render(
      <SwipeableCard
        rightSwipeAction={{
          label: "Favorite",
          icon: "\u2605",
          color: "gold",
          onTrigger: onFavorite,
        }}
        threshold={60}
      >
        <span>Server B</span>
      </SwipeableCard>,
    );

    const content = container.querySelector("[class*='content']")!;
    simulateSwipe(content as HTMLElement, 100);

    await vi.waitFor(() => expect(onFavorite).toHaveBeenCalledTimes(1));
  });

  it("does NOT trigger when swipe distance is below threshold", async () => {
    const onDelete = vi.fn();
    const { container } = render(
      <SwipeableCard
        leftSwipeAction={{
          label: "Delete",
          icon: "\u2715",
          color: "red",
          onTrigger: onDelete,
        }}
        threshold={80}
      >
        <span>Server C</span>
      </SwipeableCard>,
    );

    const content = container.querySelector("[class*='content']")!;
    // Swipe only 30px - well below 80px threshold
    simulateSwipe(content as HTMLElement, -30);

    // Give it time to settle, then confirm it was never called
    await new Promise((r) => setTimeout(r, 300));
    expect(onDelete).not.toHaveBeenCalled();
  });

  it("does NOT trigger any action when disabled", async () => {
    const onDelete = vi.fn();
    const { container } = render(
      <SwipeableCard
        leftSwipeAction={{
          label: "Delete",
          icon: "\u2715",
          color: "red",
          onTrigger: onDelete,
        }}
        threshold={60}
        disabled
      >
        <span>Server D</span>
      </SwipeableCard>,
    );

    const content = container.querySelector("[class*='content']")!;
    simulateSwipe(content as HTMLElement, -100);

    await new Promise((r) => setTimeout(r, 300));
    expect(onDelete).not.toHaveBeenCalled();
  });

  it("ignores vertical gestures (does not trigger horizontal action)", async () => {
    const onDelete = vi.fn();
    const { container } = render(
      <SwipeableCard
        leftSwipeAction={{
          label: "Delete",
          icon: "\u2715",
          color: "red",
          onTrigger: onDelete,
        }}
        threshold={60}
      >
        <span>Server E</span>
      </SwipeableCard>,
    );

    const content = container.querySelector("[class*='content']")!;
    // Primarily vertical movement
    simulateSwipe(content as HTMLElement, -20, -100);

    await new Promise((r) => setTimeout(r, 300));
    expect(onDelete).not.toHaveBeenCalled();
  });

  it("does not allow left swipe when no leftSwipeAction provided", async () => {
    const onFavorite = vi.fn();
    const { container } = render(
      <SwipeableCard
        rightSwipeAction={{
          label: "Favorite",
          icon: "\u2605",
          color: "gold",
          onTrigger: onFavorite,
        }}
        threshold={60}
      >
        <span>Server F</span>
      </SwipeableCard>,
    );

    const content = container.querySelector("[class*='content']")!;
    // Swipe LEFT when only right action exists
    simulateSwipe(content as HTMLElement, -100);

    await new Promise((r) => setTimeout(r, 300));
    expect(onFavorite).not.toHaveBeenCalled();
  });
});
