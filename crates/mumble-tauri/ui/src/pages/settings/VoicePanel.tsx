import type { AudioSettings } from "../../types";
import { Toggle, SliderField, ShortcutRecorder } from "./SharedControls";
import styles from "./SettingsPage.module.css";

export function VoicePanel({
  settings,
  onChange,
  isExpert,
}: {
  settings: AudioSettings;
  onChange: (patch: Partial<AudioSettings>) => void;
  isExpert: boolean;
}) {
  return (
    <>
      <h2 className={styles.panelTitle}>Voice</h2>

      <section className={styles.section}>
        <SliderField
          label="Voice Activation Threshold"
          hint="Audio below this level is suppressed to filter background noise."
          min={0}
          max={1}
          step={0.01}
          value={settings.vad_threshold}
          onChange={(v) => onChange({ vad_threshold: v })}
          format={(v) => `${Math.round(v * 100)}%`}
        />
      </section>

      {isExpert && (
        <>
          <section className={styles.section}>
            <SliderField
              label="Noise Gate Close Ratio"
              hint="Close threshold as a fraction of the open threshold. Lower = more aggressive gating."
              min={0.1}
              max={1}
              step={0.05}
              value={settings.noise_gate_close_ratio}
              onChange={(v) => onChange({ noise_gate_close_ratio: v })}
              format={(v) => v.toFixed(2)}
            />
          </section>

          <section className={styles.section}>
            <SliderField
              label="Hold Frames"
              hint="Frames to keep the gate open after voice drops below threshold."
              min={1}
              max={50}
              step={1}
              value={settings.hold_frames}
              onChange={(v) => onChange({ hold_frames: v })}
            />
          </section>

          <section className={styles.section}>
            <div className={styles.toggleRow}>
              <div className={styles.toggleInfo}>
                <h3 className={styles.sectionTitle}>Push-to-Talk</h3>
                <p className={styles.fieldHint}>
                  Hold a key to transmit instead of voice activation.
                </p>
              </div>
              <Toggle
                checked={settings.push_to_talk}
                onChange={() =>
                  onChange({ push_to_talk: !settings.push_to_talk })
                }
              />
            </div>
            {settings.push_to_talk && (
              <div className={styles.pttKeyRow}>
                <ShortcutRecorder
                  label="PTT Key"
                  value={settings.push_to_talk_key ?? ""}
                  onChange={(key) =>
                    onChange({ push_to_talk_key: key || null })
                  }
                />
              </div>
            )}
          </section>
        </>
      )}
    </>
  );
}
