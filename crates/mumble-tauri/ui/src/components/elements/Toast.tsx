import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import styles from "./Toast.module.css";

export interface ToastData {
  message: string;
  variant: "success" | "error";
}

interface ToastProps extends ToastData {
  /** Auto-dismiss duration in ms (default 3000). */
  duration?: number;
  onDismiss: () => void;
}

export default function Toast({ message, variant, duration = 3000, onDismiss }: ToastProps) {
  const [fadeOut, setFadeOut] = useState(false);

  useEffect(() => {
    const fadeTimer = setTimeout(() => setFadeOut(true), duration);
    const removeTimer = setTimeout(onDismiss, duration + 250);
    return () => {
      clearTimeout(fadeTimer);
      clearTimeout(removeTimer);
    };
  }, [duration, onDismiss]);

  const icon =
    variant === "success" ? (
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
        <polyline points="20 6 9 17 4 12" />
      </svg>
    ) : (
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="12" r="10" />
        <line x1="15" y1="9" x2="9" y2="15" />
        <line x1="9" y1="9" x2="15" y2="15" />
      </svg>
    );

  return createPortal(
    <div
      className={`${styles.toast} ${styles[variant]} ${fadeOut ? styles.fadeOut : ""}`}
      role="status"
      aria-live="polite"
    >
      <span className={styles.icon}>{icon}</span>
      {message}
    </div>,
    document.body,
  );
}
