import { useRef, useEffect } from "react";
import { useAppStore } from "../../store";
import styles from "./ActivityLog.module.css";

function formatTime(timestampMs: number): string {
  const d = new Date(timestampMs);
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export default function ActivityLog() {
  const serverLog = useAppStore((s) => s.serverLog);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = listRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [serverLog]);

  if (serverLog.length === 0) {
    return <p className={styles.empty}>No activity yet</p>;
  }

  return (
    <div ref={listRef} className={styles.logList}>
      {serverLog.map((entry, i) => (
        <div key={`${entry.timestamp_ms}-${i}`} className={styles.logEntry}>
          <span className={styles.logTime}>{formatTime(entry.timestamp_ms)}</span>
          <span className={styles.logMessage}>{entry.message}</span>
        </div>
      ))}
    </div>
  );
}
