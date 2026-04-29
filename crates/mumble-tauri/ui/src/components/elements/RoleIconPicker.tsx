import { useCallback, useMemo, useRef, useState, type ChangeEvent } from "react";
import { ImageEditor } from "../../pages/settings/ImageEditor";
import { useAppStore } from "../../store";
import { dataUrlToBytes, textureToDataUrl } from "../../profileFormat";
import styles from "./RoleIconPicker.module.css";

export interface RoleIconPickerProps {
  readonly value: number[] | null | undefined;
  readonly onChange: (next: number[] | null) => void;
  /**
   * Maximum icon size in bytes. When omitted, falls back to the server's
   * `max_image_message_length`, which is configurable via the
   * `imagemessagelength` setting in `mumble-server.ini`.
   */
  readonly maxBytes?: number;
  readonly disabled?: boolean;
}

/** Hard floor so we always allow a usable icon even on misconfigured servers. */
const MIN_BUDGET_BYTES = 16 * 1024;
/** Soft cap so a runaway server config can't stuff multi-MB icons into ACLs. */
const MAX_BUDGET_BYTES = 1024 * 1024;
/** Output icon resolution. Cropped square that scales nicely in role chips. */
const ICON_SIZE = 128;

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  return `${Math.round(bytes / 1024)} KiB`;
}

function clampBudget(maxBytes: number | undefined, serverMax: number): number {
  const requested = maxBytes ?? (serverMax > 0 ? serverMax : MIN_BUDGET_BYTES);
  return Math.max(MIN_BUDGET_BYTES, Math.min(MAX_BUDGET_BYTES, requested));
}

/**
 * Picks a role icon by re-using the same crop/zoom/drag editor as the
 * profile avatar. The cropped output is stored as raw bytes in
 * `AclGroup.icon` and forwarded to the server unchanged.
 */
export function RoleIconPicker({ value, onChange, maxBytes, disabled }: RoleIconPickerProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [editorSrc, setEditorSrc] = useState<string | null>(null);
  const serverMax = useAppStore((s) => s.serverConfig.max_image_message_length);
  const budget = useMemo(() => clampBudget(maxBytes, serverMax), [maxBytes, serverMax]);

  const previewSrc = useMemo(
    () => (value && value.length > 0 ? textureToDataUrl(value) : null),
    [value],
  );

  const handlePick = () => {
    if (disabled) return;
    setError(null);
    inputRef.current?.click();
  };

  const handleChange = (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => setEditorSrc(reader.result as string);
    reader.onerror = () => setError("Could not read selected file.");
    reader.readAsDataURL(file);
  };

  const handleEditorConfirm = useCallback(
    (dataUrl: string) => {
      try {
        onChange(dataUrlToBytes(dataUrl));
        setError(null);
      } catch (err) {
        console.error("Failed to encode role icon", err);
        setError("Could not process the cropped image.");
      }
      setEditorSrc(null);
    },
    [onChange],
  );

  return (
    <div className={styles.wrapper}>
      <div className={styles.preview}>
        {previewSrc ? (
          <img src={previewSrc} alt="Role icon preview" />
        ) : (
          <span className={styles.placeholder} aria-hidden="true">
            +
          </span>
        )}
      </div>
      <div className={styles.controls}>
        <div className={styles.row}>
          <button type="button" className={styles.btn} onClick={handlePick} disabled={disabled}>
            {previewSrc ? "Replace" : "Choose icon"}
          </button>
          {previewSrc && !disabled && (
            <button
              type="button"
              className={`${styles.btn} ${styles.danger}`}
              onClick={() => {
                setError(null);
                onChange(null);
              }}
            >
              Remove
            </button>
          )}
        </div>
        <span className={styles.hint}>
          Square crop, max {formatBytes(budget)} (server limit).
        </span>
        {error && <span className={styles.error}>{error}</span>}
      </div>
      <input
        ref={inputRef}
        type="file"
        accept="image/png,image/jpeg,image/webp"
        style={{ display: "none" }}
        onChange={handleChange}
      />
      {editorSrc && (
        <ImageEditor
          src={editorSrc}
          cropShape="circle"
          targetWidth={ICON_SIZE}
          targetHeight={ICON_SIZE}
          maxBytes={budget}
          onConfirm={handleEditorConfirm}
          onCancel={() => setEditorSrc(null)}
        />
      )}
    </div>
  );
}
