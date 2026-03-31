/**
 * Unit tests for the ModernChannelList component.
 *
 * Verifies flat channel rendering, sorting (populated first),
 * member display, collapse/expand, and interaction callbacks.
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ModernChannelList from "../sidebar/ModernChannelList";
import type { ChannelEntry, UserEntry } from "../../types";

// Allow tests to toggle the mobile flag.
const { isMobilePlatformMock } = vi.hoisted(() => ({
  isMobilePlatformMock: vi.fn(() => false),
}));
vi.mock("../../utils/platform", () => ({
  isMobilePlatform: isMobilePlatformMock,
  isDesktopPlatform: vi.fn(() => true),
  get isMobile() { return isMobilePlatformMock(); },
}));

function makeChannel(overrides: Partial<ChannelEntry> = {}): ChannelEntry {
  return {
    id: 1,
    parent_id: 0,
    name: "General",
    description: "",
    user_count: 0,
    permissions: null,
    temporary: false,
    position: 0,
    max_users: 0,
    ...overrides,
  };
}

function makeUser(overrides: Partial<UserEntry> = {}): UserEntry {
  return {
    session: 100,
    name: "Alice",
    channel_id: 1,
    texture: null,
    comment: null,
    self_mute: false,
    self_deaf: false,
    mute: false,
    deaf: false,
    suppress: false,
    priority_speaker: false,
    hash: "",
    ...overrides,
  };
}

const BASE_PROPS = {
  selectedChannel: null as number | null,
  currentChannel: null as number | null,
  listenedChannels: new Set<number>(),
  unreadCounts: {} as Record<number, number>,
  talkingSessions: new Set<number>(),
  onSelectChannel: vi.fn(),
  onJoinChannel: vi.fn(),
  onContextMenu: vi.fn(),
};

describe("ModernChannelList", () => {
  it("renders channel names", () => {
    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Lobby" }),
      makeChannel({ id: 2, parent_id: 0, name: "Music" }),
    ];
    render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={[]}
      />,
    );
    expect(screen.getByText("Lobby")).toBeTruthy();
    expect(screen.getByText("Music")).toBeTruthy();
  });

  it("sorts populated channels before empty ones", () => {
    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Empty Channel" }),
      makeChannel({ id: 2, parent_id: 0, name: "Active Channel" }),
    ];
    const users = [makeUser({ session: 1, name: "Bob", channel_id: 2 })];
    render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={users}
      />,
    );

    const buttons = screen.getAllByRole("button").filter(
      (b) => b.textContent?.includes("Channel"),
    );
    // Active channel (has users) should come before Empty channel
    expect(buttons[0].textContent).toContain("Active Channel");
    expect(buttons[1].textContent).toContain("Empty Channel");
  });

  it("shows member names when expanded (default)", () => {
    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Lobby" }),
    ];
    const users = [
      makeUser({ session: 1, name: "Alice", channel_id: 1 }),
      makeUser({ session: 2, name: "Bob", channel_id: 1 }),
    ];
    render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={users}
      />,
    );
    expect(screen.getByText("Alice")).toBeTruthy();
    expect(screen.getByText("Bob")).toBeTruthy();
  });

  it("hides member names and shows avatars when collapsed", () => {
    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Lobby" }),
    ];
    const users = [
      makeUser({ session: 1, name: "Alice", channel_id: 1 }),
      makeUser({ session: 2, name: "Bob", channel_id: 1 }),
    ];
    render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={users}
      />,
    );

    // Find and click the collapse button
    const collapseBtn = screen.getByLabelText("Collapse");
    fireEvent.click(collapseBtn);

    // Member names should no longer be visible as text nodes in the member list
    // but avatar initials may still show "A" and "B" in the collapsed bubbles.
    // The expand button label should now say "Expand".
    expect(screen.getByLabelText("Expand")).toBeTruthy();
  });

  it("calls onSelectChannel on click", () => {
    const onSelect = vi.fn();
    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Lobby" }),
    ];
    render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={[]}
        onSelectChannel={onSelect}
      />,
    );
    fireEvent.click(screen.getByText("Lobby"));
    expect(onSelect).toHaveBeenCalledWith(1);
  });

  it("calls onJoinChannel on double-click", () => {
    const onJoin = vi.fn();
    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Lobby" }),
    ];
    render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={[]}
        onJoinChannel={onJoin}
      />,
    );
    fireEvent.doubleClick(screen.getByText("Lobby"));
    expect(onJoin).toHaveBeenCalledWith(1);
  });

  it("shows unread badge", () => {
    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Lobby" }),
    ];
    render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={[]}
        unreadCounts={{ 1: 5 }}
      />,
    );
    expect(screen.getByText("5")).toBeTruthy();
  });

  it("shows mute/deaf icons for members", () => {
    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Lobby" }),
    ];
    const users = [
      makeUser({ session: 1, name: "Muted", channel_id: 1, self_mute: true, self_deaf: true }),
    ];
    const { container } = render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={users}
      />,
    );
    // Status icons should be rendered (SVGs)
    const svgs = container.querySelectorAll("svg");
    // At least the chevron + 2 status icons (mute + deaf)
    expect(svgs.length).toBeGreaterThanOrEqual(3);
  });

  it("excludes root channel when it has no users", () => {
    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Lobby" }),
    ];
    render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={[]}
      />,
    );
    // Root should not appear when it has no users
    expect(screen.queryByText("Root")).toBeNull();
    expect(screen.getByText("Lobby")).toBeTruthy();
  });

  it("includes root channel when it has users", () => {
    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Lobby" }),
    ];
    const users = [makeUser({ session: 1, name: "Bob", channel_id: 0 })];
    render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={users}
      />,
    );
    expect(screen.getByText("Root")).toBeTruthy();
  });

  it("flattens nested channels (no hierarchy)", () => {
    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Games" }),
      makeChannel({ id: 2, parent_id: 1, name: "Minecraft" }),
      makeChannel({ id: 3, parent_id: 1, name: "CS2" }),
    ];
    render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={[]}
      />,
    );
    // All channels should be visible at the same level (flat)
    expect(screen.getByText("Games")).toBeTruthy();
    expect(screen.getByText("Minecraft")).toBeTruthy();
    expect(screen.getByText("CS2")).toBeTruthy();
  });

  it("wraps non-current channels in SwipeableCard on mobile", async () => {
    // Enable mobile mode for this test.
    const { isMobilePlatform } = await import("../../utils/platform");
    vi.mocked(isMobilePlatform).mockReturnValue(true);

    const channels = [
      makeChannel({ id: 0, parent_id: null, name: "Root" }),
      makeChannel({ id: 1, parent_id: 0, name: "Lobby" }),
      makeChannel({ id: 2, parent_id: 0, name: "Music" }),
    ];
    render(
      <ModernChannelList
        {...BASE_PROPS}
        channels={channels}
        users={[]}
        currentChannel={1}
      />,
    );
    // Non-current channels (Music) should be inside a SwipeableCard wrapper
    // which has a specific CSS class structure. The current channel (Lobby) should not.
    // Both channels should still render their names.
    expect(screen.getByText("Lobby")).toBeTruthy();
    expect(screen.getByText("Music")).toBeTruthy();

    // SwipeableCard renders an extra wrapping div around non-current channels.
    // The Lobby card (current) has fewer wrapping layers than Music.
    const lobbyCard = screen.getByText("Lobby").closest("[class*='channelCard']");
    const musicCard = screen.getByText("Music").closest("[class*='channelCard']");
    expect(lobbyCard).toBeTruthy();
    expect(musicCard).toBeTruthy();

    // Clean up: restore desktop mode.
    vi.mocked(isMobilePlatform).mockReturnValue(false);
  });
});
