/**
 * PendingMessageItem - renders an optimistic outgoing message bubble
 * while `send_message` / `send_dm` is in flight.  Shows the message
 * body at reduced opacity with an indeterminate progress bar overlay,
 * and a "Failed - Retry / Dismiss" UI when the send rejects.
 *
 * Used for messages that contain inline media (`<img>` / `<video>`)
 * or that are large enough to be perceptibly slow on weak connections.
 */

import { useAppStore } from "../../store";
import { SafeHtml } from "../elements/SafeHtml";
import type { PendingMessage } from "../../types";
import styles from "./PendingMessageItem.module.css";

interface PendingMessageItemProps {
  readonly pending: PendingMessage;
}

export default function PendingMessageItem({ pending }: PendingMessageItemProps) {
  const dismiss = useAppStore((s) => s.dismissPendingMessage);
  const retry = useAppStore((s) => s.retryPendingMessage);
  const isFailed = pending.state === "failed";

  return (
    <div className={styles.wrapper}>
      <div
        className={`${styles.bubble} ${isFailed ? styles.bubbleFailed : styles.bubbleSending}`}
      >
        <SafeHtml html={pending.body} className={styles.body} />

        {!isFailed && (
          <div
            className={styles.progressTrack}
            role="progressbar"
            aria-label="Sending message"
            aria-valuemin={0}
            aria-valuemax={100}
          >
            <div className={styles.progressBar} />
          </div>
        )}

        <div className={styles.statusRow}>
          <span
            className={`${styles.statusLabel} ${isFailed ? styles.statusLabelError : ""}`}
          >
            {isFailed ? "Failed to send" : "Sending\u2026"}
          </span>
          <span className={styles.actions}>
            {isFailed && (
              <button
                type="button"
                className={styles.iconBtn}
                onClick={() => void retry(pending.pendingId)}
                aria-label="Retry sending"
                title="Retry"
              >
                Retry
              </button>
            )}
            <button
              type="button"
              className={styles.iconBtn}
              onClick={() => dismiss(pending.pendingId)}
              aria-label={isFailed ? "Dismiss failed message" : "Hide pending indicator"}
              title={isFailed ? "Dismiss" : "Hide"}
            >
              &#x2715;
            </button>
          </span>
        </div>

        {isFailed && pending.errorMessage && (
          <span className={styles.errorText}>{pending.errorMessage}</span>
        )}
      </div>
    </div>
  );
}
