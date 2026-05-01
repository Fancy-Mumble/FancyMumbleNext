/**
 * `useWatchStart` - shared logic to start a watch-together session
 * for a chat message that contains a startable video URL.
 *
 * Surfaces a `start()` callback and a `canStart` flag so multiple
 * entry points (the inline action button, the right-click context
 * menu, the hover action bar, the mobile bottom sheet) can offer the
 * action without duplicating the wire/protocol logic.
 *
 * Detection runs unconditionally - the `enableExternalEmbeds`
 * preference only gates the actual YouTube IFrame API mount in
 * `createPlayerAdapter`, so we surface the action even when external
 * embeds are off; the user is then shown a clear error if they try to
 * start a YouTube session.
 */

import { useCallback, useState } from "react";

import { useAppStore } from "../../../store";
import { detectVideoSource, type DetectedVideoSource } from "../../../utils/watchSourceDetect";
import { useWatchSend } from "./useWatchSend";
import { applyWatchSyncEvent } from "./watchStore";
import { markPendingAutoStart } from "./watchAutoStart";

export interface UseWatchStartResult {
  readonly detected: DetectedVideoSource | null;
  readonly canStart: boolean;
  readonly busy: boolean;
  readonly start: () => Promise<void>;
}

export function useWatchStart(
  body: string | null | undefined,
  channelId: number | null | undefined,
): UseWatchStartResult {
  const ownSession = useAppStore((s) => s.ownSession);
  const sendMessage = useAppStore((s) => s.sendMessage);
  const { sendStart } = useWatchSend();
  const [busy, setBusy] = useState(false);

  const detected = body ? detectVideoSource(body, true) : null;
  const canStart = detected !== null && channelId != null && ownSession != null;

  const start = useCallback(async () => {
    if (!detected || channelId == null || ownSession == null || busy) return;
    setBusy(true);
    try {
      const sessionId = crypto.randomUUID();
      const startEvent = {
        type: "start" as const,
        channelId,
        sourceUrl: detected.url,
        sourceKind: detected.kind,
        title: detected.title,
        hostSession: ownSession,
      };
      // Apply locally first: the server does not echo events back to
      // the sender, so without this the requester would never see its
      // own session in `watchSessions` and the card would render
      // "Waiting for watch-together session info..." forever.
      applyWatchSyncEvent({ sessionId, actor: ownSession, event: startEvent });
      // Mark for auto-start so the host's adapter begins playback on
      // mount; without this the host would have to manually click play
      // even though everyone else already shows the session as live.
      markPendingAutoStart(sessionId);
      await sendStart(sessionId, startEvent);
      await sendMessage(channelId, `<!-- FANCY_WATCH:${sessionId} -->`);
    } finally {
      setBusy(false);
    }
  }, [detected, channelId, ownSession, busy, sendStart, sendMessage]);

  return { detected, canStart, busy, start };
}
