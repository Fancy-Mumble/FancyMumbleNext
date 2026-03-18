import { useState, useCallback, useRef, useEffect, type FormEvent } from "react";
import styles from "./PasswordDialog.module.css";

interface PasswordDialogProps {
  readonly open: boolean;
  readonly onSubmit: (password: string) => void;
  readonly onCancel: () => void;
  readonly serverHost?: string;
  readonly username?: string;
}

export default function PasswordDialog({
  open,
  onSubmit,
  onCancel,
  serverHost,
  username,
}: PasswordDialogProps) {
  const [password, setPassword] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setPassword("");
      // Focus the input after the dialog appears.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const handleSubmit = useCallback(
    (e: FormEvent) => {
      e.preventDefault();
      if (password) onSubmit(password);
    },
    [password, onSubmit],
  );

  if (!open) return null;

  const target = username && serverHost
    ? `${username} on ${serverHost}`
    : serverHost ?? "this server";

  return (
    <div className={styles.overlay} role="dialog" aria-modal="true" aria-label="Password required">
      <div className={styles.dialog}>
        <div className={styles.header}>
          <h2 className={styles.title}>Password Required</h2>
          <button
            className={styles.closeBtn}
            onClick={onCancel}
            aria-label="Close"
            type="button"
          >
            ×
          </button>
        </div>

        <form className={styles.body} onSubmit={handleSubmit}>
          <p className={styles.message}>
            The server requires a password for <strong>{target}</strong>.
            Enter the password to continue connecting.
          </p>

          <div className={styles.field}>
            <label className={styles.label} htmlFor="pw-dialog-input">
              Password
            </label>
            <input
              ref={inputRef}
              id="pw-dialog-input"
              className={styles.input}
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
            />
          </div>

          <div className={styles.actions}>
            <button
              className={styles.cancelBtn}
              type="button"
              onClick={onCancel}
            >
              Cancel
            </button>
            <button
              className={styles.connectBtn}
              type="submit"
              disabled={!password}
            >
              Connect
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
