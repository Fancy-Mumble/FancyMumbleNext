import styles from "./MessageSelectionBar.module.css";

interface MessageSelectionBarProps {
  readonly count: number;
  readonly onDelete: () => void;
  readonly onCancel: () => void;
}

export default function MessageSelectionBar({
  count,
  onDelete,
  onCancel,
}: MessageSelectionBarProps) {
  return (
    <div className={styles.bar}>
      <span className={styles.count}>{count} selected</span>
      <button
        type="button"
        className={`${styles.actionBtn} ${styles.deleteBtn}`}
        onClick={onDelete}
        disabled={count === 0}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="3 6 5 6 21 6" />
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
        </svg>
        Delete ({count})
      </button>
      <button
        type="button"
        className={`${styles.actionBtn} ${styles.cancelBtn}`}
        onClick={onCancel}
      >
        Cancel
      </button>
    </div>
  );
}
