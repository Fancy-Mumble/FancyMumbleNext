import type { AudioDevice, AudioSettings } from "../../types";
import { Toggle, SliderField } from "./SharedControls";
import styles from "./SettingsPage.module.css";

const FRAME_SIZE_OPTIONS = [
  { value: 10, label: "10 ms" },
  { value: 20, label: "20 ms" },
  { value: 40, label: "40 ms" },
  { value: 60, label: "60 ms" },
];

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

      {/* ── Input Device ─────────────────────────────────── */}
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

      {/* ── Voice Activation ─────────────────────────────── */}
      <section className={styles.section}>
        <SliderField
          label="Voice Activation Threshold"
          hint="Audio below this level is treated as silence; above it is treated as speech."
          min={0}
          max={1}
          step={0.005}
          value={settings.vad_threshold}
          onChange={(v) => onChange({ vad_threshold: v })}
          format={(v) => `${(v * 100).toFixed(1)}%`}
        />
      </section>

      {/* ── Compression ──────────────────────────────────── */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Compression</h3>
        <SliderField
          label="Quality"
          hint="Higher bitrate means better audio quality but more bandwidth."
          min={8}
          max={320}
          step={8}
          value={settings.bitrate_bps / 1000}
          onChange={(v) => onChange({ bitrate_bps: v * 1000 })}
          format={(v) => `${v} kb/s`}
        />
        <div className={styles.field}>
          <div className={styles.fieldRow}>
            <label className={styles.fieldLabel}>Audio per packet</label>
            <span className={styles.sliderValue}>
              {settings.frame_size_ms} ms
            </span>
          </div>
          <p className={styles.fieldHint}>
            Smaller values reduce latency; larger values are more
            bandwidth-efficient.
          </p>
          <div className={styles.radioGroup}>
            {FRAME_SIZE_OPTIONS.map((opt) => (
              <label key={opt.value} className={styles.radioLabel}>
                <input
                  type="radio"
                  name="frame_size_ms"
                  value={opt.value}
                  checked={settings.frame_size_ms === opt.value}
                  onChange={() => onChange({ frame_size_ms: opt.value })}
                />
                {opt.label}
              </label>
            ))}
          </div>
        </div>
      </section>

      {/* ── Audio Processing ─────────────────────────────── */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Audio Processing</h3>

        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <span className={styles.fieldLabel}>Auto Gain</span>
            <p className={styles.fieldHint}>
              Automatically adjusts microphone volume for consistent levels.
            </p>
          </div>
          <Toggle
            checked={settings.auto_gain}
            onChange={() => onChange({ auto_gain: !settings.auto_gain })}
          />
        </div>

        {settings.auto_gain && (
          <SliderField
            label="Max Amplification"
            hint="Maximum boost the auto-gain controller can apply."
            min={1}
            max={40}
            step={1}
            value={settings.max_gain_db}
            onChange={(v) => onChange({ max_gain_db: v })}
            format={(v) => `${v} dB`}
          />
        )}
      </section>

      {/* ── Noise Suppression ────────────────────────────── */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Noise Suppression</h3>

        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <span className={styles.fieldLabel}>Noise Gate</span>
            <p className={styles.fieldHint}>
              Silences audio below the voice activation threshold to remove
              background noise between speech.
            </p>
          </div>
          <Toggle
            checked={settings.noise_suppression}
            onChange={() =>
              onChange({ noise_suppression: !settings.noise_suppression })
            }
          />
        </div>
      </section>

      {/* ── Expert settings ──────────────────────────────── */}
      {isExpert && (
        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>Expert</h3>
          <SliderField
            label="Gate Close Ratio"
            hint="Close-threshold as a fraction of the open-threshold (hysteresis)."
            min={0.1}
            max={1}
            step={0.05}
            value={settings.noise_gate_close_ratio}
            onChange={(v) => onChange({ noise_gate_close_ratio: v })}
            format={(v) => `${(v * 100).toFixed(0)}%`}
          />
          <SliderField
            label="Hold Frames"
            hint="How many frames to keep the gate open after audio drops below threshold."
            min={1}
            max={50}
            step={1}
            value={settings.hold_frames}
            onChange={(v) => onChange({ hold_frames: v })}
            format={(v) => `${v}`}
          />
        </section>
      )}
    </>
  );
}
