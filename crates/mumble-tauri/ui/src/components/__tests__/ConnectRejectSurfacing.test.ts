/**
 * Regression: connection rejections must surface an error rather than
 * leaving the UI stuck on a "connecting" skeleton.  When `pendingConnect`
 * is set AND there is no active session yet (initial-connect race),
 * rejections are treated as targeting that attempt even if the new
 * session id has not yet been registered as `activeServerId`.
 *
 * Conversely, when the user is ALREADY connected to a server and a new
 * connect to a *different* server fails, the rejection / disconnect
 * event must NOT clobber the currently-active tab's state.  It is
 * recorded in `sessionErrors` so the failed tab shows the reason when
 * the user switches to it.
 */

import { describe, it, expect, beforeEach } from "vitest";
import { useAppStore } from "../../store";

interface RejectEvent {
  serverId?: string | null;
  reason: string;
  reject_type: number | null;
}

interface DisconnectEvent {
  serverId?: string | null;
  reason: string | null;
}

/**
 * Replicates the early-bailout check from the "connection-rejected"
 * listener in store.ts.  Returns `true` if the listener should treat
 * the event as belonging to the active session and proceed with
 * cleanup; `false` if it should bail (non-active session).
 */
function shouldHandleReject(event: RejectEvent): boolean {
  const eventServerId = event.serverId ?? null;
  const { activeServerId, pendingConnect } = useAppStore.getState();
  const pendingFallbackApplies =
    pendingConnect !== null &&
    activeServerId === null &&
    (eventServerId === null || eventServerId !== activeServerId);
  return !(
    eventServerId !== null &&
    eventServerId !== activeServerId &&
    !pendingFallbackApplies
  );
}

/** Same logic for the "server-disconnected" listener. */
function shouldHandleDisconnect(event: DisconnectEvent): boolean {
  const eventServerId = event.serverId ?? null;
  const { activeServerId, pendingConnect } = useAppStore.getState();
  const pendingFallbackApplies =
    pendingConnect !== null &&
    activeServerId === null &&
    (eventServerId === null || eventServerId !== activeServerId);
  return (
    (eventServerId !== null && eventServerId === activeServerId) ||
    pendingFallbackApplies
  );
}

beforeEach(() => {
  useAppStore.setState({
    activeServerId: null,
    pendingConnect: null,
  });
});

describe("connect-time rejection surfacing", () => {
  it("ignores rejections for non-active sessions when no connect is pending", () => {
    useAppStore.setState({ activeServerId: "srv-A", pendingConnect: null });
    const handled = shouldHandleReject({
      serverId: "srv-B",
      reason: "kicked",
      reject_type: null,
    });
    expect(handled).toBe(false);
  });

  it("handles rejection for a pending connect when no active session exists yet", () => {
    // Initial connect: no prior tabs, activeServerId is null,
    // pendingConnect was set by the connect action.
    useAppStore.setState({
      activeServerId: null,
      pendingConnect: {
        host: "h",
        port: 64738,
        username: "u",
        certLabel: null,
      },
    });
    const handled = shouldHandleReject({
      serverId: "srv-newly-registered",
      reason: "Wrong certificate or password for existing user",
      reject_type: null,
    });
    expect(handled).toBe(true);
  });

  it("does NOT clobber an active session when a *different* connect attempt is rejected", () => {
    // Regression: user is already connected to A; tries to add server B
    // (or reconnect from a tab that left other sessions intact).  The
    // rejection arrives tagged with the new session's id.  We must NOT
    // treat it as belonging to the active tab, otherwise A's tab
    // becomes unusable with a misleading "Disconnected" overlay.
    useAppStore.setState({
      activeServerId: "srv-A-existing",
      pendingConnect: {
        host: "h",
        port: 64738,
        username: "u",
        certLabel: null,
      },
    });
    const handled = shouldHandleReject({
      serverId: "srv-B-failed",
      reason: "Connection refused",
      reject_type: null,
    });
    expect(handled).toBe(false);
  });

  it("still handles rejections for the active session as before", () => {
    useAppStore.setState({ activeServerId: "srv-A", pendingConnect: null });
    const handled = shouldHandleReject({
      serverId: "srv-A",
      reason: "kicked",
      reject_type: null,
    });
    expect(handled).toBe(true);
  });
});

describe("connect-time disconnect surfacing", () => {
  it("treats a disconnect event for a pending connect as active when no active session exists yet", () => {
    useAppStore.setState({
      activeServerId: null,
      pendingConnect: {
        host: "h",
        port: 64738,
        username: "u",
        certLabel: null,
      },
    });
    const handled = shouldHandleDisconnect({
      serverId: "srv-newly-registered",
      reason: "Connection to server was lost.",
    });
    expect(handled).toBe(true);
  });

  it("does NOT treat background-tab disconnects as active when no connect is pending", () => {
    useAppStore.setState({
      activeServerId: "srv-A",
      pendingConnect: null,
    });
    const handled = shouldHandleDisconnect({
      serverId: "srv-B",
      reason: "kicked",
    });
    expect(handled).toBe(false);
  });

  it("does NOT clobber an active tab when a *different* pending connect fails", () => {
    // Regression: user connected to A and starts a connect to B which
    // fails.  The disconnect event for B must not surface as a
    // "Disconnected" overlay on A's tab.
    useAppStore.setState({
      activeServerId: "srv-A-existing",
      pendingConnect: {
        host: "h",
        port: 64738,
        username: "u",
        certLabel: null,
      },
    });
    const handled = shouldHandleDisconnect({
      serverId: "srv-B-failed",
      reason: "Connection refused",
    });
    expect(handled).toBe(false);
  });
});

