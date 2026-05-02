/**
 * Regression tests for Phase F: per-session unread badges in the
 * server tabs bar.  The Tauri "unread-changed" / "dm-unread-changed"
 * listeners route counts to either the active session's
 * `unreadCounts` / `dmUnreadCounts` (existing behaviour) or to the
 * per-tab `sessionUnreadTotals` map (new) for non-active sessions.
 *
 * These tests reproduce that routing logic in isolation so the slice
 * contract is locked in even if the listener wiring is refactored.
 */

import { describe, it, expect, beforeEach } from "vitest";
import { useAppStore } from "../../store";

type UnreadEvent = {
  unreads: Record<number, number>;
  serverId?: string | null;
};

/** Replicates the body of the "unread-changed" listener. */
function applyUnreadChanged(event: UnreadEvent): void {
  const { activeServerId } = useAppStore.getState();
  const eventServerId = event.serverId ?? null;
  const total = Object.values(event.unreads).reduce((a, b) => a + b, 0);
  if (eventServerId && eventServerId !== activeServerId) {
    useAppStore.setState((prev) => {
      const next = { ...prev.sessionUnreadTotals };
      const prevDm = next[`${eventServerId}:dm`] ?? 0;
      next[`${eventServerId}:ch`] = total;
      next[eventServerId] = total + prevDm;
      return { sessionUnreadTotals: next };
    });
    return;
  }
  useAppStore.setState({ unreadCounts: event.unreads });
}

/** Replicates the body of the "dm-unread-changed" listener. */
function applyDmUnreadChanged(event: UnreadEvent): void {
  const { activeServerId } = useAppStore.getState();
  const eventServerId = event.serverId ?? null;
  const total = Object.values(event.unreads).reduce((a, b) => a + b, 0);
  if (eventServerId && eventServerId !== activeServerId) {
    useAppStore.setState((prev) => {
      const next = { ...prev.sessionUnreadTotals };
      const prevCh = next[`${eventServerId}:ch`] ?? 0;
      next[`${eventServerId}:dm`] = total;
      next[eventServerId] = total + prevCh;
      return { sessionUnreadTotals: next };
    });
    return;
  }
  useAppStore.setState({ dmUnreadCounts: event.unreads });
}

beforeEach(() => {
  useAppStore.setState({
    activeServerId: "active-srv",
    unreadCounts: {},
    dmUnreadCounts: {},
    sessionUnreadTotals: {},
  });
});

describe("Phase F: per-session unread routing", () => {
  it("active-session unreads land in unreadCounts, not in sessionUnreadTotals", () => {
    applyUnreadChanged({ unreads: { 1: 3, 2: 1 }, serverId: "active-srv" });
    const state = useAppStore.getState();
    expect(state.unreadCounts).toEqual({ 1: 3, 2: 1 });
    expect(state.sessionUnreadTotals).toEqual({});
  });

  it("non-active-session channel unreads accumulate in sessionUnreadTotals", () => {
    applyUnreadChanged({ unreads: { 7: 4, 8: 2 }, serverId: "other-srv" });
    const state = useAppStore.getState();
    expect(state.unreadCounts).toEqual({});
    expect(state.sessionUnreadTotals["other-srv"]).toBe(6);
    expect(state.sessionUnreadTotals["other-srv:ch"]).toBe(6);
  });

  it("non-active-session DM unreads sum with channel unreads in the tab total", () => {
    applyUnreadChanged({ unreads: { 7: 4 }, serverId: "other-srv" });
    applyDmUnreadChanged({ unreads: { 99: 3, 100: 1 }, serverId: "other-srv" });
    const state = useAppStore.getState();
    expect(state.sessionUnreadTotals["other-srv:ch"]).toBe(4);
    expect(state.sessionUnreadTotals["other-srv:dm"]).toBe(4);
    expect(state.sessionUnreadTotals["other-srv"]).toBe(8);
  });

  it("missing serverId falls back to active-session behaviour", () => {
    applyUnreadChanged({ unreads: { 1: 5 } });
    const state = useAppStore.getState();
    expect(state.unreadCounts).toEqual({ 1: 5 });
    expect(state.sessionUnreadTotals).toEqual({});
  });

  it("multiple non-active sessions are tracked independently", () => {
    applyUnreadChanged({ unreads: { 1: 2 }, serverId: "srv-a" });
    applyUnreadChanged({ unreads: { 1: 7 }, serverId: "srv-b" });
    const state = useAppStore.getState();
    expect(state.sessionUnreadTotals["srv-a"]).toBe(2);
    expect(state.sessionUnreadTotals["srv-b"]).toBe(7);
  });

  it("a fresh non-active update for a session replaces its prior channel count", () => {
    applyUnreadChanged({ unreads: { 1: 5 }, serverId: "srv-a" });
    applyUnreadChanged({ unreads: { 1: 0, 2: 1 }, serverId: "srv-a" });
    const state = useAppStore.getState();
    expect(state.sessionUnreadTotals["srv-a"]).toBe(1);
  });
});
