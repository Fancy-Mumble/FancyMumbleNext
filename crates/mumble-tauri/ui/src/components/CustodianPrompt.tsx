import { useEffect, useCallback } from "react";
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
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor"
              strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        <div className={styles.body}>
          <p className={styles.description}>{description}</p>

          {isChange && (
            <div className={styles.warning}>
              <svg className={styles.warningIcon} viewBox="0 0 24 24" fill="none" stroke="currentColor"
                strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
                <line x1="12" y1="9" x2="12" y2="13" />
                <line x1="12" y1="17" x2="12.01" y2="17" />
              </svg>
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
                  <svg className={styles.custodianIcon} viewBox="0 0 24 24" fill="none" stroke="currentColor"
                    strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                  </svg>
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
                  <svg className={styles.custodianIcon} viewBox="0 0 24 24" fill="none" stroke="currentColor"
                    strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                  </svg>
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
                  <svg className={styles.custodianIcon} viewBox="0 0 24 24" fill="none" stroke="currentColor"
                    strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                  </svg>
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
