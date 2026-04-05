import { useEffect, useCallback } from "react";
import CloseIcon from "../../assets/icons/action/close.svg?react";
import WarningIcon from "../../assets/icons/status/warning.svg?react";
import ShieldIcon from "../../assets/icons/status/shield.svg?react";
import styles from "./CustodianPrompt.module.css";

interface Custodian {
  readonly hash: string;
  readonly name?: string;
}

interface CustodianPromptProps {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly onConfirm: () => void;
  readonly custodians: Custodian[];
  readonly isFirstJoin: boolean;
  readonly removedCustodians?: Custodian[];
  readonly addedCustodians?: Custodian[];
}

export default function CustodianPrompt({
  open,
  onClose,
  onConfirm,
  custodians,
  isFirstJoin,
  removedCustodians,
  addedCustodians,
}: CustodianPromptProps) {
  // Close on Escape.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  const handleConfirm = useCallback(() => {
    onConfirm();
    onClose();
  }, [onConfirm, onClose]);

  if (!open) return null;

  const isChange = !isFirstJoin;
  const title = isChange ? "Channel Authority Changed" : "Key Custodians";
  const description = isChange
    ? "The channel's key custodians have changed. New custodians will not be trusted until you accept this change."
    : `This channel is managed by ${custodians.length} key custodian(s). Review and confirm to enable accelerated key verification.`;

  return (
    <dialog className={styles.overlay} open aria-label={title}>
      <div className={styles.dialog}>
        <div className={styles.header}>
          <h3 className={styles.title}>{title}</h3>
          <button className={styles.closeBtn} onClick={onClose} aria-label="Close">
            <CloseIcon width={16} height={16} />
          </button>
        </div>

        <div className={styles.body}>
          <p className={styles.description}>{description}</p>

          {isChange && (
            <div className={styles.warning}>
              <WarningIcon className={styles.warningIcon} aria-hidden="true" />
              <span>
                Until you accept, only previously pinned custodians are trusted
                for key verification. New custodians cannot bypass consensus.
              </span>
            </div>
          )}

          {/* Show changes for custodian-change scenario */}
          {isChange && addedCustodians && addedCustodians.length > 0 && (
            <ul className={styles.custodianList}>
              {addedCustodians.map((c) => (
                <li key={c.hash} className={`${styles.custodianItem} ${styles.changeAdded}`}>
                  <ShieldIcon className={styles.custodianIcon} aria-hidden="true" />
                  <span className={styles.custodianName}>{c.name ?? "Unknown"}</span>
                  <span className={styles.custodianHash}>{c.hash.slice(0, 12)}...</span>
                  <span className={`${styles.changeBadge} ${styles.badgeAdded}`}>Added</span>
                </li>
              ))}
            </ul>
          )}

          {isChange && removedCustodians && removedCustodians.length > 0 && (
            <ul className={styles.custodianList}>
              {removedCustodians.map((c) => (
                <li key={c.hash} className={`${styles.custodianItem} ${styles.changeRemoved}`}>
                  <ShieldIcon className={styles.custodianIcon} aria-hidden="true" />
                  <span className={styles.custodianName}>{c.name ?? "Unknown"}</span>
                  <span className={styles.custodianHash}>{c.hash.slice(0, 12)}...</span>
                  <span className={`${styles.changeBadge} ${styles.badgeRemoved}`}>Removed</span>
                </li>
              ))}
            </ul>
          )}

          {/* Current custodian list for first-join scenario */}
          {isFirstJoin && (
            <ul className={styles.custodianList}>
              {custodians.map((c) => (
                <li key={c.hash} className={styles.custodianItem}>
                  <ShieldIcon className={styles.custodianIcon} aria-hidden="true" />
                  <span className={styles.custodianName}>{c.name ?? "Unknown"}</span>
                  <span className={styles.custodianHash}>{c.hash.slice(0, 12)}...</span>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className={styles.footer}>
          <button className={styles.btnSecondary} onClick={onClose}>
            {isChange ? "Dismiss" : "Later"}
          </button>
          <button className={styles.btnPrimary} onClick={handleConfirm}>
            {isChange ? "Accept" : "Confirm"}
          </button>
        </div>
      </div>
    </dialog>
  );
}
