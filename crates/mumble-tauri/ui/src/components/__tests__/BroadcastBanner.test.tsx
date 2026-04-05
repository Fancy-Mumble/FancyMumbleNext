/**
 * Tests for the BroadcastBanner component.
 *
 * Verifies rendering, watch button interaction, and dismiss behaviour.
 */
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { BroadcastBanner } from "../chat/ScreenShareViewer";

describe("BroadcastBanner", () => {
  it("renders nothing when no broadcasters", () => {
    const { container } = render(
      <BroadcastBanner broadcasters={[]} onWatch={vi.fn()} />,
    );
    expect(container.innerHTML).toBe("");
  });

  it("shows broadcaster name", () => {
    render(
      <BroadcastBanner
        broadcasters={[{ session: 42, name: "Alice" }]}
        onWatch={vi.fn()}
      />,
    );
    expect(screen.getByText("Alice")).toBeTruthy();
    expect(screen.getByText(/sharing their screen/)).toBeTruthy();
  });

  it("shows multiple broadcaster banners", () => {
    render(
      <BroadcastBanner
        broadcasters={[
          { session: 1, name: "Alice" },
          { session: 2, name: "Bob" },
        ]}
        onWatch={vi.fn()}
      />,
    );
    expect(screen.getByText("Alice")).toBeTruthy();
    expect(screen.getByText("Bob")).toBeTruthy();
  });

  it("calls onWatch with the correct session when Watch is clicked", () => {
    const onWatch = vi.fn();
    render(
      <BroadcastBanner
        broadcasters={[{ session: 42, name: "Alice" }]}
        onWatch={onWatch}
      />,
    );
    fireEvent.click(screen.getByText("Watch"));
    expect(onWatch).toHaveBeenCalledWith(42);
  });

  it("dismisses a banner when dismiss button is clicked", () => {
    render(
      <BroadcastBanner
        broadcasters={[
          { session: 1, name: "Alice" },
          { session: 2, name: "Bob" },
        ]}
        onWatch={vi.fn()}
      />,
    );

    // Dismiss Alice's banner.
    const dismissButtons = screen.getAllByTitle("Dismiss");
    fireEvent.click(dismissButtons[0]);

    // Alice's banner should be gone, Bob's should remain.
    expect(screen.queryByText("Alice")).toBeNull();
    expect(screen.getByText("Bob")).toBeTruthy();
  });
});
