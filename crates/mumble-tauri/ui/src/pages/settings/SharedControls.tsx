import { useState, useCallback } from "react";
import { eventToShortcut } from "./shortcutHelpers";
import styles from "./SettingsPage.module.css";

export function Toggle({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: () => void;
}) {
  return (
    <button
      type="button"
      className={`${styles.toggle} ${checked ? styles.toggleOn : ""}`}
      onClick={onChange}
      role="switch"
      aria-checked={checked}
    >
      <span className={styles.toggleKnob} />
    </button>
  );
}

export function SliderField({
  label,
  hint,
  min,
  max,
  step,
  value,
  onChange,
  format,
}: {
  label: string;
  hint?: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (v: number) => void;
  format?: (v: number) => string;
}) {
  const display = format ? format(value) : String(value);
  return (
    <div className={styles.field}>
      <div className={styles.fieldRow}>
        <label className={styles.fieldLabel}>{label}</label>
        <span className={styles.sliderValue}>{display}</span>
      </div>
      {hint && <p className={styles.fieldHint}>{hint}</p>}
      <input
        type="range"
        className={styles.slider}
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
      />
    </div>
  );
}

export function ShortcutRecorder({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (shortcut: string) => void;
}) {
  const [recording, setRecording] = useState(false);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const shortcut = eventToShortcut(e);
      if (shortcut) {
        onChange(shortcut);
        setRecording(false);
      }
    },
    [onChange],
  );

  return (
    <div className={styles.recorderRow}>
      <span className={styles.recorderLabel}>{label}</span>
      {recording ? (
        <input
          className={`${styles.recorderInput} ${styles.recorderActive}`}
          autoFocus
          readOnly
          placeholder="Press a key combo…"
          onKeyDown={handleKeyDown}
          onBlur={() => setRecording(false)}
        />
      ) : (
        <button
          type="button"
          className={styles.recorderBtn}
          onClick={() => setRecording(true)}
        >
          {value || "Not set"}
        </button>
      )}
      {value && (
        <button
          type="button"
          className={styles.clearBtn}
          onClick={() => onChange("")}
          title="Clear shortcut"
        >
          ✕
        </button>
      )}
    </div>
  );
}
