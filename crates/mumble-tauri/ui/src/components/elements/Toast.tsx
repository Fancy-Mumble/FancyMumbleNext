import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import CheckIcon from "../../assets/icons/status/check.svg?react";
import ErrorCircleIcon from "../../assets/icons/status/error-circle.svg?react";
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
      <CheckIcon width={16} height={16} strokeWidth={2.5} />
    ) : (
      <ErrorCircleIcon width={16} height={16} strokeWidth={2.5} />
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
