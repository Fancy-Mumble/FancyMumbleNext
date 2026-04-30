import { useId, useRef, type ChangeEvent } from "react";
import styles from "./RoleColorPicker.module.css";

export interface RoleColorPickerProps {
  readonly value: string | null | undefined;
  readonly onChange: (next: string | null) => void;
  readonly presets?: readonly string[];
  readonly disabled?: boolean;
}

const DEFAULT_PRESETS: readonly string[] = [
  "#5865f2", // blurple
  "#3ba55d", // green
  "#faa61a", // amber
  "#ed4245", // red
  "#eb459e", // pink
  "#9b59b6", // purple
  "#1abc9c", // teal
  "#e67e22", // orange
  "#95a5a6", // gray
];

function isValidColor(input: string): boolean {
  return /^#([0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(input.trim());
}

/**
 * Compact color picker used for role customization. Combines a native color
 * input with a hex text field and a row of preset swatches.
 */
export function RoleColorPicker({ value, onChange, presets = DEFAULT_PRESETS, disabled }: RoleColorPickerProps) {
  const inputId = useId();
  const colorInputRef = useRef<HTMLInputElement>(null);
  const current = value ?? "";

  const handleSwatchClick = () => {
    if (disabled) return;
    colorInputRef.current?.click();
  };

  const handleColorChange = (e: ChangeEvent<HTMLInputElement>) => {
    onChange(e.target.value);
  };

  const handleHexChange = (e: ChangeEvent<HTMLInputElement>) => {
    const raw = e.target.value.trim();
    if (!raw) {
      onChange(null);
      return;
    }
    onChange(isValidColor(raw) ? raw : raw);
  };

  return (
    <div className={styles.wrapper}>
      <div className={styles.row}>
        <button
          type="button"
          className={`${styles.swatch} ${!current ? styles.empty : ""}`}
          style={current ? ({ "--swatch-color": current } as React.CSSProperties) : undefined}
          onClick={handleSwatchClick}
          aria-label="Open color picker"
          disabled={disabled}
        />
        <input
          ref={colorInputRef}
          id={inputId}
          type="color"
          value={isValidColor(current) ? current : "#5865f2"}
          onChange={handleColorChange}
          style={{ display: "none" }}
        />
        <input
          type="text"
          className={styles.input}
          placeholder="#5865f2"
          value={current}
          onChange={handleHexChange}
          disabled={disabled}
        />
        {current && !disabled && (
          <button type="button" className={styles.clearBtn} onClick={() => onChange(null)}>
            Clear
          </button>
        )}
      </div>
      <div className={styles.presets}>
        {presets.map((p) => (
          <button
            key={p}
            type="button"
            className={`${styles.presetSwatch} ${current.toLowerCase() === p.toLowerCase() ? styles.active : ""}`}
            style={{ background: p }}
            onClick={() => onChange(p)}
            aria-label={`Use color ${p}`}
            disabled={disabled}
          />
        ))}
      </div>
    </div>
  );
}
