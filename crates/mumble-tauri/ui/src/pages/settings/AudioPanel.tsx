import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AudioDevice, AudioSettings } from "../../types";
import { Toggle, SliderField, ShortcutRecorder } from "./SharedControls";
import styles from "./SettingsPage.module.css";

const FRAME_SIZE_OPTIONS = [
  { value: 10, label: "10 ms" },
  { value: 20, label: "20 ms" },
  { value: 40, label: "40 ms" },
  { value: 60, label: "60 ms" },
];

/** Peak-hold decay: percentage-points per second. */
const PEAK_DECAY_PER_SEC = 80;

function VuMeter({ rms, peak, threshold }: Readonly<{ rms: number; peak: number; threshold: number }>) {
  const fillRef = useRef<HTMLDivElement>(null);
  const peakRef = useRef<HTMLDivElement>(null);
  const threshRef = useRef<HTMLDivElement>(null);
  const heldPeak = useRef(0);
  const lastTime = useRef(performance.now());
  const rafId = useRef(0);

  useEffect(() => {
    const now = performance.now();
    const dt = (now - lastTime.current) / 1000; // seconds since last update
    lastTime.current = now;

    const scaledPeak = Math.min(peak * 500, 100);

    // Decay held peak by time elapsed, then snap up if new peak is higher.
    heldPeak.current = Math.max(0, heldPeak.current - PEAK_DECAY_PER_SEC * dt);
    if (scaledPeak > heldPeak.current) {
      heldPeak.current = scaledPeak;
    }

    // Write directly to the DOM to avoid extra React re-renders.
    cancelAnimationFrame(rafId.current);
    rafId.current = requestAnimationFrame(() => {
      const rmsPercent = Math.min(rms * 500, 100);
      if (fillRef.current) fillRef.current.style.width = `${rmsPercent}%`;
      if (peakRef.current) peakRef.current.style.left = `${heldPeak.current}%`;
      if (threshRef.current) {
        const threshPercent = Math.min(threshold * 500, 100);
        threshRef.current.style.left = `${threshPercent}%`;
      }
    });

    return () => cancelAnimationFrame(rafId.current);
  }, [rms, peak, threshold]);

  return (
    <div className={styles.vuMeter}>
      <div className={styles.vuTrack}>
        <div className={styles.vuFill} ref={fillRef} />
        <div className={styles.vuPeak} ref={peakRef} />
        <div className={styles.vuThreshold} ref={threshRef} title={`Threshold: ${(threshold * 100).toFixed(1)}%`} />
      </div>
      <div className={styles.vuLabels}>
        <span>-60</span>
        <span>-40</span>
        <span>-20</span>
        <span>0 dB</span>
      </div>
    </div>
  );
}

export function AudioPanel({
  devices,
  outputDevices,
  settings,
  onChange,
  isExpert,
}: Readonly<{
  devices: AudioDevice[];
  outputDevices: AudioDevice[];
  settings: AudioSettings;
  onChange: (patch: Partial<AudioSettings>) => void;
  isExpert: boolean;
}>) {
  const [micTesting, setMicTesting] = useState(false);
  const micTestingRef = useRef(false);
  // Store latest amplitude in a ref to avoid re-rendering on every event.
  const amplitudeRef = useRef({ rms: 0, peak: 0 });
  // A state counter bumped at display-rate to trigger VuMeter updates.
  const [ampTick, setAmpTick] = useState(0);
  const rafHandle = useRef(0);

  const toggleMicTest = useCallback(async () => {
    if (micTestingRef.current) {
      await invoke("stop_mic_test").catch(() => {});
      setMicTesting(false);
      micTestingRef.current = false;
      amplitudeRef.current = { rms: 0, peak: 0 };
      setAmpTick((t) => t + 1);
    } else {
      try {
        await invoke("start_mic_test");
        setMicTesting(true);
        micTestingRef.current = true;
      } catch (e) {
        console.error("Mic test failed:", e);
      }
    }
  }, []);

  // Listen for amplitude events while mic test is active.
  // Buffer into a ref and flush to React at display rate.
  useEffect(() => {
    if (!micTesting) return;
    const unlisten = listen<{ rms: number; peak: number }>(
      "mic-amplitude",
      (event) => {
        amplitudeRef.current = event.payload;
        cancelAnimationFrame(rafHandle.current);
        rafHandle.current = requestAnimationFrame(() =>
          setAmpTick((t) => t + 1),
        );
      },
    );
    return () => {
      cancelAnimationFrame(rafHandle.current);
      unlisten.then((f) => f());
    };
  }, [micTesting]);

  // Stop mic test on unmount.
  useEffect(() => {
    return () => {
      if (micTestingRef.current) {
        invoke("stop_mic_test").catch(() => {});
      }
    };
  }, []);

  // Listen for backend-driven threshold updates (auto-calibration).
  useEffect(() => {
    const unlisten = listen<number>("vad-threshold-updated", (event) => {
      onChange({ vad_threshold: event.payload });
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [onChange]);

  // Read amplitude from ref (the ampTick dependency triggers re-reads).
  void ampTick; // used only to trigger re-render
  const amplitude = amplitudeRef.current;

  return (
    <>
      <h2 className={styles.panelTitle}>Voice</h2>

      {/* -- Input & Output Devices (side by side) ---------- */}
      <section className={styles.section}>
        <div className={styles.deviceColumns}>
          {/* Left: Microphone */}
          <div className={styles.deviceColumn}>
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
              {devices.map((d, i) => (
                <option key={`in-${i}-${d.name}`} value={d.name}>
                  {d.name}
                  {d.is_default ? " (default)" : ""}
                </option>
              ))}
            </select>
            <SliderField
              label="Microphone Volume"
              min={0}
              max={2}
              step={0.01}
              value={settings.input_volume}
              onChange={(v) => onChange({ input_volume: v })}
              format={(v) => `${Math.round(v * 100)}%`}
            />
          </div>

          {/* Right: Speaker */}
          <div className={styles.deviceColumn}>
            <h3 className={styles.sectionTitle}>Output Device</h3>
            <select
              className={styles.select}
              value={settings.selected_output_device ?? ""}
              onChange={(e) =>
                onChange({
                  selected_output_device:
                    e.target.value === "" ? null : e.target.value,
                })
              }
            >
              <option value="">System default</option>
              {outputDevices.map((d, i) => (
                <option key={`out-${i}-${d.name}`} value={d.name}>
                  {d.name}
                  {d.is_default ? " (default)" : ""}
                </option>
              ))}
            </select>
            <SliderField
              label="Speaker Volume"
              min={0}
              max={2}
              step={0.01}
              value={settings.output_volume}
              onChange={(v) => onChange({ output_volume: v })}
              format={(v) => `${Math.round(v * 100)}%`}
            />
          </div>
        </div>
      </section>

      {/* -- Activation Mode ---------------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Activation Mode</h3>
        <p className={styles.fieldHint}>
          Choose how your microphone is activated.
        </p>
        <div className={styles.radioGroup}>
          <label className={styles.radioLabel}>
            <input
              type="radio"
              name="activation_mode"
              checked={!settings.push_to_talk && settings.noise_suppression}
              onChange={() =>
                onChange({ push_to_talk: false, noise_suppression: true })
              }
            />
            Voice Activation
          </label>
          <label className={styles.radioLabel}>
            <input
              type="radio"
              name="activation_mode"
              checked={!settings.push_to_talk && !settings.noise_suppression}
              onChange={() =>
                onChange({
                  push_to_talk: false,
                  noise_suppression: false,
                  auto_input_sensitivity: false,
                })
              }
            />
            Continuous
          </label>
          <label className={styles.radioLabel}>
            <input
              type="radio"
              name="activation_mode"
              checked={settings.push_to_talk}
              onChange={() =>
                onChange({
                  push_to_talk: true,
                  noise_suppression: false,
                  auto_input_sensitivity: false,
                })
              }
            />
            Push to Talk
          </label>
        </div>
      </section>

      {/* -- Voice Activation Settings ------------------------------- */}
      {!settings.push_to_talk && settings.noise_suppression && (
        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>Voice Activation</h3>

          <div className={styles.toggleRow}>
            <div className={styles.toggleInfo}>
              <span className={styles.fieldLabel}>Auto Sensitivity</span>
              <p className={styles.fieldHint}>
                Automatically adjusts the activation threshold based on your
                ambient noise level.
              </p>
            </div>
            <Toggle
              checked={settings.auto_input_sensitivity}
              onChange={() =>
                onChange({
                  auto_input_sensitivity: !settings.auto_input_sensitivity,
                })
              }
            />
          </div>

          {!settings.auto_input_sensitivity && (
            <SliderField
              label="Threshold"
              hint="Audio below this level is treated as silence; above it is treated as speech."
              min={0}
              max={1}
              step={0.005}
              value={settings.vad_threshold}
              onChange={(v) => onChange({ vad_threshold: v })}
              format={(v) => `${(v * 100).toFixed(1)}%`}
            />
          )}

          {settings.auto_input_sensitivity && (
            <div className={styles.field}>
              <div className={styles.fieldRow}>
                <span className={styles.fieldLabel}>Current Threshold</span>
                <span className={styles.sliderValue}>
                  {(settings.vad_threshold * 100).toFixed(1)}%
                </span>
              </div>
            </div>
          )}

          {/* Calibrate: starts mic test with live VU meter */}
          <div className={styles.micTestRow}>
            <button
              type="button"
              className={`${styles.micTestBtn} ${micTesting ? styles.micTestActive : ""}`}
              onClick={toggleMicTest}
            >
              {micTesting ? "Stop" : "Calibrate"}
            </button>
            {micTesting && <VuMeter rms={amplitude.rms} peak={amplitude.peak} threshold={settings.vad_threshold} />}
          </div>
        </section>
      )}

      {/* -- Push-to-Talk Key ---------------------------------------- */}
      {settings.push_to_talk && (
        <section className={styles.section}>
          <div className={styles.pttKeyRow}>
            <ShortcutRecorder
              label="PTT Key"
              value={settings.push_to_talk_key ?? ""}
              onChange={(key) =>
                onChange({ push_to_talk_key: key || null })
              }
            />
          </div>
        </section>
      )}

      {/* -- Audio Processing ------------------------------- */}
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

      {/* -- Compression ------------------------------------ */}
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
            <span className={styles.fieldLabel}>Audio per packet</span>
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

      {/* -- Expert settings -------------------------------- */}
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
