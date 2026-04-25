import { WarningIcon } from "../../icons";
import type { PersistenceMode } from "../../types";
import styles from "./KeyShareWarningDialog.module.css";

interface KeyShareWarningDialogProps {
  readonly open: boolean;
  readonly peerName: string;
  readonly persistenceMode: PersistenceMode;
  readonly totalStored: number;
  readonly onConfirm: () => void;
  readonly onCancel: () => void;
}

function describeAccess(mode: PersistenceMode, totalStored: number): string {
  if (mode === "FANCY_V1_FULL_ARCHIVE") {
    const count = totalStored > 0 ? ` (${totalStored} stored messages)` : "";
    return `This channel uses full archive mode. Sharing the key grants access to the entire message history${count}.`;
  }
  return "Sharing the encryption key grants access to encrypted messages in this channel.";
}

export default function KeyShareWarningDialog({
  open,
  peerName,
  persistenceMode,
  totalStored,
  onConfirm,
  onCancel,
}: KeyShareWarningDialogProps) {
  if (!open) return null;

  return (
    <div className={styles.overlay} role="dialog" aria-modal="true" aria-label="Share encryption key">
      <div className={styles.dialog}>
        <div className={styles.header}>
          <h2 className={styles.title}>Share Encryption Key</h2>
          <button
            className={styles.closeBtn}
            onClick={onCancel}
            aria-label="Close"
            type="button"
          >
            &times;
          </button>
        </div>

        <div className={styles.body}>
          <div className={styles.warningBanner}>
            <WarningIcon className={styles.warningIcon} width={20} height={20} />
            <span>{describeAccess(persistenceMode, totalStored)}</span>
          </div>

          <p className={styles.message}>
            Are you sure you want to share the encryption key with <strong>{peerName}</strong>?
            This cannot be undone.
          </p>

          <div className={styles.actions}>
            <button
              className={styles.cancelBtn}
              type="button"
              onClick={onCancel}
            >
              Cancel
            </button>
            <button
              className={styles.confirmBtn}
              type="button"
              onClick={onConfirm}
            >
              Share Key
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
