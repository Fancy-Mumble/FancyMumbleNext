/**
 * Unit tests for the KebabMenu component.
 *
 * Verifies rendering, opening/closing, item clicks, disabled items,
 * and active styling.
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import KebabMenu, { type KebabMenuItem } from "../elements/KebabMenu";

function makeItems(overrides: Partial<KebabMenuItem>[] = []): KebabMenuItem[] {
  const defaults: KebabMenuItem[] = [
    { id: "poll", label: "Create poll", onClick: vi.fn() },
    { id: "silence", label: "Mute channel", onClick: vi.fn() },
  ];
  return defaults.map((item, i) => ({
    ...item,
    ...(overrides[i] ?? {}),
  }));
}

describe("KebabMenu", () => {
  it("renders the trigger button", () => {
    render(<KebabMenu items={makeItems()} />);
    expect(screen.getByRole("button", { name: "More options" })).toBeTruthy();
  });

  it("menu is hidden by default", () => {
    render(<KebabMenu items={makeItems()} />);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("opens the menu on trigger click", () => {
    render(<KebabMenu items={makeItems()} />);
    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    expect(screen.getByRole("menu")).toBeTruthy();
    expect(screen.getByText("Create poll")).toBeTruthy();
    expect(screen.getByText("Mute channel")).toBeTruthy();
  });

  it("calls item onClick and closes the menu", () => {
    const items = makeItems();
    render(<KebabMenu items={items} />);
    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    fireEvent.click(screen.getByText("Create poll"));
    expect(items[0].onClick).toHaveBeenCalledOnce();
    // Menu should be closed after clicking an item
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("closes the menu on backdrop click", () => {
    render(<KebabMenu items={makeItems()} />);
    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    expect(screen.getByRole("menu")).toBeTruthy();
    // The backdrop is the first sibling before the menu
    const backdrop = screen.getByRole("menu").previousElementSibling as HTMLElement;
    fireEvent.click(backdrop);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("closes on Escape key", () => {
    render(<KebabMenu items={makeItems()} />);
    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    expect(screen.getByRole("menu")).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("does not call onClick for disabled items", () => {
    const items = makeItems([{ disabled: true }]);
    render(<KebabMenu items={items} />);
    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    const pollBtn = screen.getByText("Create poll");
    expect(pollBtn.closest("button")!.disabled).toBe(true);
  });

  it("applies active styling to active items", () => {
    const items = makeItems([{}, { active: true }]);
    render(<KebabMenu items={items} />);
    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    const silenceBtn = screen.getByText("Mute channel").closest("button")!;
    expect(silenceBtn.className).toContain("Active");
  });

  it("uses custom ariaLabel", () => {
    render(<KebabMenu items={makeItems()} ariaLabel="Channel options" />);
    expect(screen.getByRole("button", { name: "Channel options" })).toBeTruthy();
  });
});
