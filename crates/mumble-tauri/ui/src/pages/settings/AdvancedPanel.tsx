import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { UserMode, TimeFormat } from "../../types";
import { Toggle } from "./SharedControls";
import styles from "./SettingsPage.module.css";

const TIME_FORMAT_OPTIONS: { value: TimeFormat; label: string }[] = [
  { value: "auto", label: "Auto (follow system)" },
  { value: "12h", label: "12-hour (AM/PM)" },
  { value: "24h", label: "24-hour" },
];

export function AdvancedPanel({
  userMode,
  klipyApiKey,
  enableNotifications,
  disableDualPath,
  timeFormat,
  convertToLocalTime,
  onToggleMode,
  onKlipyApiKeyChange,
  onToggleNotifications,
  onToggleDualPath,
  onTimeFormatChange,
  onConvertToLocalTimeChange,
  onToggleDeveloperMode,
  onReset,
}: {
  userMode: UserMode;
  klipyApiKey: string;
  enableNotifications: boolean;
  disableDualPath: boolean;
  timeFormat: TimeFormat;
  convertToLocalTime: boolean;
  onToggleMode: () => void;
  onKlipyApiKeyChange: (key: string) => void;
  onToggleNotifications: () => void;
  onToggleDualPath: () => void;
  onTimeFormatChange: (fmt: TimeFormat) => void;
  onConvertToLocalTimeChange: () => void;
  onToggleDeveloperMode: () => void;
  onReset: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [debugLogging, setDebugLogging] = useState(false);

  const toggleDebugLogging = async () => {
    const next = !debugLogging;
    const filter = next ? "debug" : "info";
    try {
      await invoke("set_log_level", { filter });
      setDebugLogging(next);
    } catch (e) {
      console.error("Failed to set log level:", e);
    }
  };

  return (
    <>
      <h2 className={styles.panelTitle}>Advanced</h2>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>Expert Mode</h3>
            <p className={styles.fieldHint}>
              {userMode === "normal"
                ? "Streamlined - we handle the technical details for you."
                : "Full control - advanced audio options, custom ports and labels are visible."}
            </p>
          </div>
          <Toggle checked={userMode !== "normal"} onChange={onToggleMode} />
        </div>
      </section>

      {userMode !== "normal" && (
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
            placeholder="klipy_xxxxxxxx..."
            autoComplete="off"
            spellCheck={false}
          />
        </section>
      )}

      {userMode !== "normal" && (
        <section className={styles.section}>
          <div className={styles.toggleRow}>
            <div className={styles.toggleInfo}>
              <h3 className={styles.sectionTitle}>Developer Mode</h3>
              <p className={styles.fieldHint}>
                Show debug statistics (message counts, offloaded content, memory
                usage) in the server info panel.
              </p>
            </div>
            <Toggle
              checked={userMode === "developer"}
              onChange={onToggleDeveloperMode}
            />
          </div>
        </section>
      )}

      {userMode === "developer" && (
        <section className={styles.section}>
          <div className={styles.toggleRow}>
            <div className={styles.toggleInfo}>
              <h3 className={styles.sectionTitle}>Debug Logging</h3>
              <p className={styles.fieldHint}>
                Enable verbose debug logging in the Rust backend. Useful for
                diagnosing protocol and connection issues. Resets to info level
                on app restart.
              </p>
            </div>
            <Toggle checked={debugLogging} onChange={toggleDebugLogging} />
          </div>
        </section>
      )}

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>Notifications</h3>
            <p className={styles.fieldHint}>
              Show native notifications for new messages when the app is in the
              background.
            </p>
          </div>
          <Toggle
            checked={enableNotifications}
            onChange={onToggleNotifications}
          />
        </div>
      </section>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>
              Disable dual-path sending
            </h3>
            <p className={styles.fieldHint}>
              When enabled, encrypted channels replace the plain-text message
              with a placeholder so the server never sees the real content.
              Legacy clients without E2EE support will only see
              &quot;[Encrypted message]&quot;.
            </p>
          </div>
          <Toggle checked={disableDualPath} onChange={onToggleDualPath} />
        </div>
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Time Display</h3>
        <p className={styles.fieldHint}>
          Choose how timestamps are formatted in chat messages.
        </p>

        <label className={styles.fieldLabel}>Time Format</label>
        <div className={styles.optionGrid}>
          {TIME_FORMAT_OPTIONS.map((opt) => (
            <button
              key={opt.value}
              type="button"
              className={`${styles.optionCard} ${timeFormat === opt.value ? styles.optionCardSelected : ""}`}
              onClick={() => onTimeFormatChange(opt.value)}
            >
              <span className={styles.optionLabel}>{opt.label}</span>
            </button>
          ))}
        </div>

        <div className={styles.toggleRow} style={{ marginTop: 12 }}>
          <div className={styles.toggleInfo}>
            <label className={styles.fieldLabel}>Convert to local time</label>
            <p className={styles.fieldHint}>
              When enabled, timestamps are displayed in your local timezone.
              When disabled, times are shown in UTC.
            </p>
          </div>
          <Toggle
            checked={convertToLocalTime}
            onChange={onConvertToLocalTimeChange}
          />
        </div>
      </section>

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
