import type { AudioDevice, AudioSettings } from "../../types";
import { Toggle, SliderField } from "./SharedControls";
import styles from "./SettingsPage.module.css";

export function AudioPanel({
  devices,
  settings,
  onChange,
  isExpert,
}: {
  devices: AudioDevice[];
  settings: AudioSettings;
  onChange: (patch: Partial<AudioSettings>) => void;
  isExpert: boolean;
}) {
  return (
    <>
      <h2 className={styles.panelTitle}>Audio</h2>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Input Device</h3>
        <select
          className={styles.select}
          value={settings.selected_device ?? ""}
          onChange={(e) =>
            onChange({
              selected_device: e.target.value === "" ? null : e.target.value,
            })
          }
        >
          <option value="">System default</option>
          {devices.map((d) => (
            <option key={d.name} value={d.name}>
              {d.name}
              {d.is_default ? " (default)" : ""}
            </option>
          ))}
        </select>
      </section>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>Auto Gain</h3>
            <p className={styles.fieldHint}>
              Automatically adjusts microphone volume for consistent levels.
            </p>
          </div>
          <Toggle
            checked={settings.auto_gain}
            onChange={() => onChange({ auto_gain: !settings.auto_gain })}
          />
        </div>
      </section>

      {isExpert && (
        <section className={styles.section}>
          <SliderField
            label="Max Gain"
            hint="Maximum boost the auto-gain controller can apply."
            min={1}
            max={40}
            step={1}
            value={settings.max_gain_db}
            onChange={(v) => onChange({ max_gain_db: v })}
            format={(v) => `${v} dB`}
          />
        </section>
      )}
    </>
  );
}
