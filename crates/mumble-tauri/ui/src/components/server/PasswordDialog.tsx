import { useState, useCallback, useRef, useEffect, type FormEvent } from "react";
import styles from "./PasswordDialog.module.css";

interface PasswordDialogProps {
  readonly open: boolean;
  readonly onSubmit: (password: string, savePassword: boolean) => void;
  readonly onCancel: () => void;
  readonly serverHost?: string;
  readonly username?: string;
  readonly error?: string | null;
  readonly showSaveOption?: boolean;
  readonly onChangeUsername?: (newUsername: string) => void;
}

export default function PasswordDialog({
  open,
  onSubmit,
  onCancel,
  serverHost,
  username,
  error,
  showSaveOption,
  onChangeUsername,
}: PasswordDialogProps) {
  const [password, setPassword] = useState("");
  const [savePassword, setSavePassword] = useState(false);
  const [editingUsername, setEditingUsername] = useState(false);
  const [usernameDraft, setUsernameDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const usernameInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setPassword("");
      setSavePassword(false);
      setEditingUsername(false);
      setUsernameDraft(username ?? "");
      // Focus the input after the dialog appears.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open, username]);

  useEffect(() => {
    if (editingUsername) {
      requestAnimationFrame(() => usernameInputRef.current?.focus());
    }
  }, [editingUsername]);

  const handleSubmit = useCallback(
    (e: FormEvent) => {
      e.preventDefault();
      if (password) onSubmit(password, savePassword);
    },
    [password, savePassword, onSubmit],
  );

  const handleChangeUsername = useCallback(
    (e: FormEvent) => {
      e.preventDefault();
      const trimmed = usernameDraft.trim();
      if (!trimmed || trimmed === username || !onChangeUsername) return;
      onChangeUsername(trimmed);
    },
    [usernameDraft, username, onChangeUsername],
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

        {editingUsername && onChangeUsername ? (
          <form className={styles.body} onSubmit={handleChangeUsername}>
            <p className={styles.message}>
              Connect as a different user on <strong>{serverHost}</strong>.
            </p>
            <div className={styles.field}>
              <label className={styles.label} htmlFor="pw-dialog-username">
                New username
              </label>
              <input
                ref={usernameInputRef}
                id="pw-dialog-username"
                className={styles.input}
                type="text"
                value={usernameDraft}
                onChange={(e) => setUsernameDraft(e.target.value)}
                autoComplete="username"
                autoCapitalize="off"
                autoCorrect="off"
                spellCheck={false}
              />
            </div>
            <div className={styles.actions}>
              <button
                className={styles.cancelBtn}
                type="button"
                onClick={() => setEditingUsername(false)}
              >
                Back
              </button>
              <button
                className={styles.connectBtn}
                type="submit"
                disabled={!usernameDraft.trim() || usernameDraft.trim() === username}
              >
                Reconnect
              </button>
            </div>
          </form>
        ) : (
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

            {onChangeUsername && (
              <button
                type="button"
                className={styles.changeUserBtn}
                onClick={() => setEditingUsername(true)}
              >
                Use a different username
              </button>
            )}

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
        )}
      </div>
    </div>
  );
}
