/**
 * ServerEditSheet - inline form for editing an existing saved server.
 *
 * Rendered inside a MobileBottomSheet on mobile or as a dialog on desktop.
 * Reuses the glass input style from ConnectPage.
 */

import { useEffect, useState, type FormEvent } from "react";
import type { SavedServer } from "../types";
import { getServerPassword, setServerPassword } from "../serverStorage";
import { isMobilePlatform } from "../utils/platform";
import MobileBottomSheet from "./MobileBottomSheet";
import styles from "./ServerEditSheet.module.css";

interface Props {
  server: SavedServer;
  onSave: (id: string, patch: Partial<Omit<SavedServer, "id">>) => void;
  onClose: () => void;
}

function EditForm({ server, onSave, onClose }: Readonly<Props>) {
  const [label, setLabel] = useState(server.label || "");
  const [host, setHost] = useState(server.host);
  const [port, setPort] = useState(String(server.port));
  const [username, setUsername] = useState(server.username);
  const [password, setPassword] = useState("");
  const [hasStoredPassword, setHasStoredPassword] = useState(false);
  const [clearPassword, setClearPassword] = useState(false);

  useEffect(() => {
    getServerPassword(server.id).then((pw) => {
      if (pw) {
        setHasStoredPassword(true);
      }
    });
  }, [server.id]);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!host.trim() || !username.trim()) return;

    // Handle password changes
    if (clearPassword) {
      await setServerPassword(server.id, null);
    } else if (password) {
      await setServerPassword(server.id, password);
    }

    onSave(server.id, {
      label: label.trim() || host.trim(),
      host: host.trim(),
      port: Number.parseInt(port) || 64738,
      username: username.trim(),
    });
  };

  return (
    <form className={styles.form} onSubmit={handleSubmit}>
      <h3 className={styles.title}>Edit Server</h3>

      <label className={styles.fieldLabel}>
        Label
        <input
          className={styles.input}
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          placeholder="My Server"
        />
      </label>

      <label className={styles.fieldLabel}>
        Host
        <input
          className={styles.input}
          value={host}
          onChange={(e) => setHost(e.target.value)}
          placeholder="mumble.example.com"
          required
        />
      </label>

      <label className={styles.fieldLabel}>
        Port
        <input
          className={styles.input}
          type="number"
          value={port}
          onChange={(e) => setPort(e.target.value)}
          placeholder="64738"
          min={1}
          max={65535}
        />
      </label>

      <label className={styles.fieldLabel}>
        Username
        <input
          className={styles.input}
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          placeholder="Username"
          required
        />
      </label>

      <label className={styles.fieldLabel}>
        Password
        {hasStoredPassword && !clearPassword ? (
          <div className={styles.storedPassword}>
            <span className={styles.storedLabel}>Saved password stored</span>
            <button
              type="button"
              className={styles.clearPasswordBtn}
              onClick={() => setClearPassword(true)}
            >
              Remove
            </button>
          </div>
        ) : (
          <input
            className={styles.input}
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder={clearPassword ? "Enter new password or leave empty" : "Leave empty if not required"}
            autoComplete="new-password"
          />
        )}
      </label>

      <div className={styles.actions}>
        <button type="button" className={styles.cancelBtn} onClick={onClose}>
          Cancel
        </button>
        <button type="submit" className={styles.saveBtn}>
          Save
        </button>
      </div>
    </form>
  );
}

export default function ServerEditSheet({ server, onSave, onClose }: Readonly<Props>) {
  if (isMobilePlatform()) {
    return (
      <MobileBottomSheet open onClose={onClose} ariaLabel="Close server editor">
        <EditForm server={server} onSave={onSave} onClose={onClose} />
      </MobileBottomSheet>
    );
  }

  // Desktop: portal overlay dialog (similar pattern to ConfirmDialog)
  return (
    <div className={styles.overlay} onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className={styles.dialog}>
        <EditForm server={server} onSave={onSave} onClose={onClose} />
      </div>
    </div>
  );
}
