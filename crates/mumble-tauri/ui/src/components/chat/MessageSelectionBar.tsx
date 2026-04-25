import { TrashIcon } from "../../icons";
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
        <TrashIcon width={14} height={14} />
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
