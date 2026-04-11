import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import CheckIcon from "../../assets/icons/status/check.svg?react";
import ErrorCircleIcon from "../../assets/icons/status/error-circle.svg?react";
import CloseIcon from "../../assets/icons/action/close.svg?react";
import styles from "./Toast.module.css";

export interface ToastData {
  message: string;
  variant: "success" | "error";
  /** Auto-dismiss duration in ms (default 4000). */
  duration?: number;
  /** Show a manual dismiss button (default false). */
  dismissible?: boolean;
}

interface ToastProps extends ToastData {
  onDismiss: () => void;
}

export default function Toast({ message, variant, duration = 4000, dismissible, onDismiss }: ToastProps) {
  const [fadeOut, setFadeOut] = useState(false);

  useEffect(() => {
    const fadeTimer = setTimeout(() => setFadeOut(true), duration);
    const removeTimer = setTimeout(onDismiss, duration + 300);
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
      role="alert"
      aria-live="assertive"
    >
      <span className={styles.icon}>{icon}</span>
      <span className={styles.message}>{message}</span>
      {dismissible && (
        <button
          type="button"
          className={styles.dismissBtn}
          onClick={onDismiss}
          aria-label="Dismiss"
        >
          <CloseIcon width={14} height={14} />
        </button>
      )}
    </div>,
    document.body,
  );
}
