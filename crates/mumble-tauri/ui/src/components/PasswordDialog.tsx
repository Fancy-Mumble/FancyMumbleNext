import { useState, useCallback, useRef, useEffect, type FormEvent } from "react";
import styles from "./PasswordDialog.module.css";

interface PasswordDialogProps {
  readonly open: boolean;
  readonly onSubmit: (password: string, savePassword: boolean) => void;
  readonly onCancel: () => void;
  readonly serverHost?: string;
  readonly username?: string;
  readonly error?: string | null;
  /** Whether to show the "Save password" checkbox. Hidden when there's no saved server to attach to. */
  readonly showSaveOption?: boolean;
}

export default function PasswordDialog({
  open,
  onSubmit,
  onCancel,
  serverHost,
  username,
  error,
  showSaveOption,
}: PasswordDialogProps) {
  const [password, setPassword] = useState("");
  const [savePassword, setSavePassword] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setPassword("");
      setSavePassword(false);
      // Focus the input after the dialog appears.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const handleSubmit = useCallback(
    (e: FormEvent) => {
      e.preventDefault();
      if (password) onSubmit(password, savePassword);
    },
    [password, savePassword, onSubmit],
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
          {error && (
            <p className={styles.error}>{error}</p>
          )}
          <p className={styles.message}>
            {error
              ? <>Try again for <strong>{target}</strong>.</>
              : <>The server requires a password for <strong>{target}</strong>. Enter the password to continue connecting.</>}
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
            {showSaveOption && (
              <label className={styles.checkboxLabel}>
                <input
                  type="checkbox"
                  checked={savePassword}
                  onChange={(e) => setSavePassword(e.target.checked)}
                  className={styles.checkbox}
                />
                Save password
              </label>
            )}
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
