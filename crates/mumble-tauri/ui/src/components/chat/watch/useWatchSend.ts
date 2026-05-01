import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

import type { WatchSyncEvent } from "./watchTypes";

/**
 * Hook returning thin wrappers around the `send_watch_sync` Tauri
 * command.  All helpers send a single FancyWatchSync event to the
 * server which relays it to the channel; the server fills in `actor`
 * before relaying.
 */
export function useWatchSend() {
  const send = useCallback(async (sessionId: string, event: WatchSyncEvent) => {
    try {
      await invoke("send_watch_sync", { sessionId, event });
    } catch (err) {
      console.error("[watch] send failed", event.type, err);
    }
  }, []);

  return {
    send,
    sendStart: (sessionId: string, args: Extract<WatchSyncEvent, { type: "start" }>) =>
      send(sessionId, args),
    sendState: (sessionId: string, args: Extract<WatchSyncEvent, { type: "state" }>) =>
      send(sessionId, args),
    sendJoin: (sessionId: string, session: number) =>
      send(sessionId, { type: "join", session }),
    sendLeave: (sessionId: string, session: number) =>
      send(sessionId, { type: "leave", session }),
    sendStateRequest: (sessionId: string) =>
      send(sessionId, { type: "stateRequest" }),
    sendEnd: (sessionId: string) => send(sessionId, { type: "end" }),
    sendHostTransfer: (sessionId: string, newHostSession: number) =>
      send(sessionId, { type: "hostTransfer", newHostSession }),
  };
}
