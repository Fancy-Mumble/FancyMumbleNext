/**
 * Regression: connection rejections must surface an error rather than
 * leaving the UI stuck on a "connecting" skeleton.  When `pendingConnect`
 * is set, rejections are treated as targeting that attempt even if
 * `activeServerId` has not caught up to the new session id yet.
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
  const isPending = pendingConnect !== null;
  return !(
    eventServerId !== null &&
    eventServerId !== activeServerId &&
    !isPending
  );
}

/** Same logic for the "server-disconnected" listener. */
function shouldHandleDisconnect(event: DisconnectEvent): boolean {
  const eventServerId = event.serverId ?? null;
  const { activeServerId, pendingConnect } = useAppStore.getState();
  return (
    (eventServerId !== null && eventServerId === activeServerId) ||
    pendingConnect !== null
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

  it("handles rejection for a pending connect even if activeServerId is stale", () => {
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

  it("handles reconnect rejection for a pending connect with a different stale activeServerId", () => {
    // Reconnect flow: ChatPage.handleReconnect removed the old session
    // and started a new one.  activeServerId may have rebound to some
    // other tab; pendingConnect is still set.
    useAppStore.setState({
      activeServerId: "srv-other-tab",
      pendingConnect: {
        host: "h",
        port: 64738,
        username: "u",
        certLabel: null,
      },
    });
    const handled = shouldHandleReject({
      serverId: "srv-fresh-after-reconnect",
      reason: "Connection refused",
      reject_type: null,
    });
    expect(handled).toBe(true);
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
  it("treats a disconnect event for a pending connect as active even with stale activeServerId", () => {
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
});
