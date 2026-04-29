/**
 * Unit tests for the RoleChip component.
 *
 * Verifies that the chip renders the role name, applies the color CSS
 * variable when a color is provided, and renders the icon image when
 * icon bytes are provided.
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { RoleChip } from "../elements/RoleChip";

describe("RoleChip", () => {
  it("renders the role name", () => {
    render(<RoleChip name="moderator" />);
    expect(screen.getByText("moderator")).toBeTruthy();
  });

  it("applies the color CSS variable when a color is given", () => {
    const { container } = render(<RoleChip name="admin" color="#ff5500" />);
    const chip = container.firstChild as HTMLElement;
    expect(chip.style.getPropertyValue("--role-color")).toBe("#ff5500");
  });

  it("renders an icon image when icon bytes are provided", () => {
    const png = [137, 80, 78, 71, 13, 10, 26, 10];
    const { container } = render(<RoleChip name="vip" icon={png} />);
    const img = container.querySelector("img");
    expect(img).not.toBeNull();
    expect(img!.getAttribute("src")?.startsWith("data:image")).toBe(true);
  });

  it("renders as a button and fires onClick", () => {
    const fn = vi.fn();
    render(<RoleChip name="dev" onClick={fn} />);
    fireEvent.click(screen.getByRole("button", { name: /dev/i }));
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("does not render the icon img when icon is null", () => {
    const { container } = render(<RoleChip name="plain" />);
    expect(container.querySelector("img")).toBeNull();
  });
});
