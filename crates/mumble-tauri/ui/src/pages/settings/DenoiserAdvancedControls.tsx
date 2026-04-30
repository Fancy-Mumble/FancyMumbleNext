import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  AudioSettings,
  DenoiserParamSpec,
  NoiseSuppressionAlgorithm,
} from "../../types";
import { SliderField } from "./SharedControls";
import styles from "./SettingsPage.module.css";

/**
 * Renders one slider per knob exposed by the currently-selected
 * denoiser algorithm.  The schema is fetched from the Rust side via
 * the `get_denoiser_param_specs` Tauri command so the UI stays in
 * sync with whichever algorithms the protocol crate ships.
 *
 * Renders nothing for algorithms that have no tunable knobs (e.g.
 * `none`, `rnnoise`).
 */
export function DenoiserAdvancedControls({
  algorithm,
  settings,
  onChange,
}: Readonly<{
  algorithm: NoiseSuppressionAlgorithm;
  settings: AudioSettings;
  onChange: (patch: Partial<AudioSettings>) => void;
}>) {
  const [specs, setSpecs] = useState<DenoiserParamSpec[]>([]);

  useEffect(() => {
    let cancelled = false;
    invoke<DenoiserParamSpec[]>("get_denoiser_param_specs", { algorithm })
      .then((s) => {
        if (!cancelled) setSpecs(s);
      })
      .catch(() => {
        if (!cancelled) setSpecs([]);
      });
    return () => {
      cancelled = true;
    };
  }, [algorithm]);

  if (specs.length === 0) return null;

  const params = settings.denoiser_params ?? {};

  return (
    <div className={styles.field}>
      <span className={styles.fieldLabel}>Algorithm parameters</span>
      <p className={styles.fieldHint}>
        Fine-tune the selected denoiser. Defaults are tuned for everyday voice
        chat; only adjust these if the result sounds wrong on your hardware.
      </p>
      {specs.map((spec) => {
        const value = params[spec.id] ?? spec.default;
        const label = spec.unit ? `${spec.label} (${spec.unit})` : spec.label;
        return (
          <SliderField
            key={spec.id}
            label={label}
            hint={spec.description}
            value={value}
            min={spec.min}
            max={spec.max}
            step={spec.step}
            onChange={(v) =>
              onChange({
                denoiser_params: { ...params, [spec.id]: v },
              })
            }
          />
        );
      })}
    </div>
  );
}
