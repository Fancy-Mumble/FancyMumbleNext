import { Toggle } from "./SharedControls";
import styles from "./SettingsPage.module.css";

export function PrivacyPanel({
  disableDualPath,
  disableReadReceipts,
  disableTypingIndicators,
  onToggleDualPath,
  onToggleReadReceipts,
  onToggleTypingIndicators,
}: {
  disableDualPath: boolean;
  disableReadReceipts: boolean;
  disableTypingIndicators: boolean;
  onToggleDualPath: () => void;
  onToggleReadReceipts: () => void;
  onToggleTypingIndicators: () => void;
}) {
  return (
    <>
      <h2 className={styles.panelTitle}>Privacy</h2>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>
              Disable dual-path sending
            </h3>
            <p className={styles.fieldHint}>
              When enabled, encrypted channels replace the plain-text message
              with a placeholder so the server never sees the real content.
              Legacy clients without E2EE support will only see
              &quot;[Encrypted message]&quot;.
            </p>
          </div>
          <Toggle checked={disableDualPath} onChange={onToggleDualPath} />
        </div>
      </section>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>
              Disable read receipts
            </h3>
            <p className={styles.fieldHint}>
              When enabled, other users will not see that you have read their
              messages. You will also not see read receipts from others.
            </p>
          </div>
          <Toggle checked={disableReadReceipts} onChange={onToggleReadReceipts} />
        </div>
      </section>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>
              Disable typing indicators
            </h3>
            <p className={styles.fieldHint}>
              When enabled, you will not send typing indicators to others
              and you will not see when others are typing.
            </p>
          </div>
          <Toggle checked={disableTypingIndicators} onChange={onToggleTypingIndicators} />
        </div>
      </section>
    </>
  );
}
