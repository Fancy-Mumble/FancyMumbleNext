import { useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { completeSetup } from "../preferencesStorage";
import type { UserMode } from "../types";
import styles from "./WelcomePage.module.css";

export default function WelcomePage({ onComplete }: { onComplete?: () => void }) {
  const navigate = useNavigate();
  const [mode, setMode] = useState<UserMode>("normal");
  const [username, setUsername] = useState("");
  const [saving, setSaving] = useState(false);

  const handleSubmit = useCallback(
    async (e: { preventDefault: () => void }) => {
      e.preventDefault();
      if (!username.trim()) return;
      setSaving(true);
      await completeSetup(mode, username.trim());
      // Generate a default certificate for TLS client auth.
      try {
        await invoke("generate_certificate", { label: "default" });
      } catch {
        // Non-fatal - the user can still connect anonymously.
      }
      onComplete?.();
      navigate("/", { replace: true });
    },
    [mode, username, navigate, onComplete],
  );

  return (
    <div className={styles.page}>
      <div className={styles.card}>
        {/* Logo */}
        <div className={styles.logo}>
          <div className={styles.logoIcon}>M</div>
          <h1 className={styles.title}>Welcome</h1>
        </div>

        <form onSubmit={handleSubmit}>
          {/* Username */}
          <div className={styles.field}>
            <label htmlFor="welcome-username" className={styles.label}>
              Username
            </label>
            <input
              id="welcome-username"
              className={styles.input}
              type="text"
              placeholder="Your name"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoFocus
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="off"
              spellCheck={false}
            />
          </div>

          {/* Mode selection */}
          <div className={styles.field}>
            <span className={styles.label}>Interface</span>
            <div className={styles.modeToggle} role="radiogroup">
              <button
                type="button"
                className={`${styles.modeOption} ${mode === "normal" ? styles.modeActive : ""}`}
                onClick={() => setMode("normal")}
                aria-pressed={mode === "normal"}
              >
                <span className={styles.modeTitle}>Simple</span>
                <span className={styles.modeHint}>Just connect and talk</span>
              </button>
              <button
                type="button"
                className={`${styles.modeOption} ${mode === "expert" ? styles.modeActive : ""}`}
                onClick={() => setMode("expert")}
                aria-pressed={mode === "expert"}
              >
                <span className={styles.modeTitle}>Advanced</span>
                <span className={styles.modeHint}>Full control</span>
              </button>
            </div>
          </div>

          <button
            className={styles.button}
            type="submit"
            disabled={!username.trim() || saving}
          >
            {saving ? "Starting…" : "Get started"}
          </button>
        </form>
      </div>
    </div>
  );
}
