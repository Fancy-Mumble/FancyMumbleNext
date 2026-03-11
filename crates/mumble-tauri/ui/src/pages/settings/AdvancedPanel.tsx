import { useState } from "react";
import type { UserMode } from "../../types";
import { Toggle } from "./SharedControls";
import styles from "./SettingsPage.module.css";

export function AdvancedPanel({
  userMode,
  klipyApiKey,
  onToggleMode,
  onKlipyApiKeyChange,
  onReset,
}: {
  userMode: UserMode;
  klipyApiKey: string;
  onToggleMode: () => void;
  onKlipyApiKeyChange: (key: string) => void;
  onReset: () => void;
}) {
  const [confirming, setConfirming] = useState(false);

  return (
    <>
      <h2 className={styles.panelTitle}>Advanced</h2>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>Expert Mode</h3>
            <p className={styles.fieldHint}>
              {userMode === "expert"
                ? "Full control - advanced audio options, custom ports and labels are visible."
                : "Streamlined - we handle the technical details for you."}
            </p>
          </div>
          <Toggle checked={userMode === "expert"} onChange={onToggleMode} />
        </div>
      </section>

      {userMode === "expert" && (
        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>Klipy API Key</h3>
          <p className={styles.fieldHint}>
            Provide your own{" "}
            <a
              href="https://klipy.com"
              target="_blank"
              rel="noopener noreferrer"
              style={{ color: "var(--accent)" }}
            >
              Klipy
            </a>{" "}
            API key for GIF/sticker search. Leave empty to use the built-in key.
          </p>
          <input
            type="password"
            className={styles.input}
            value={klipyApiKey}
            onChange={(e) => onKlipyApiKeyChange(e.target.value)}
            placeholder="klipy_xxxxxxxx…"
            autoComplete="off"
            spellCheck={false}
          />
        </section>
      )}

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Danger Zone</h3>
        <p className={styles.fieldHint}>
          Delete all saved data - servers, preferences, and certificates - and
          return to the welcome screen.
        </p>
        {confirming ? (
          <div className={styles.confirmBox}>
            <p className={styles.confirmText}>
              Are you sure? This cannot be undone.
            </p>
            <div className={styles.confirmBtns}>
              <button
                type="button"
                className={styles.dangerBtn}
                onClick={onReset}
              >
                Yes, reset everything
              </button>
              <button
                type="button"
                className={styles.ghostBtn}
                onClick={() => setConfirming(false)}
              >
                Cancel
              </button>
            </div>
          </div>
        ) : (
          <button
            type="button"
            className={styles.dangerBtn}
            onClick={() => setConfirming(true)}
          >
            Reset all data
          </button>
        )}
      </section>
    </>
  );
}
