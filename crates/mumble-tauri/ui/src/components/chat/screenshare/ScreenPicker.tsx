import { useEffect, useCallback, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import type { CaptureSourceInfo } from "../../../types";
import styles from "./ScreenPicker.module.css";

interface ScreenPickerProps {
  onSelect: (sourceIndex: number) => void;
  onCancel: () => void;
}

export default function ScreenPicker({ onSelect, onCancel }: ScreenPickerProps) {
  const [sources, setSources] = useState<CaptureSourceInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke<CaptureSourceInfo[]>("list_capture_sources")
      .then((result) => {
        if (cancelled) return;
        setSources(result);
        const primary = result.find((s) => s.is_primary);
        if (primary) setSelected(primary.index);
        else if (result.length > 0) setSelected(result[0].index);
      })
      .catch((e) => {
        console.error("[screen-picker] list_capture_sources failed:", e);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => { cancelled = true; };
  }, []);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
      if (e.key === "Enter" && selected !== null) onSelect(selected);
    },
    [onCancel, onSelect, selected],
  );

  useEffect(() => {
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  const handleOverlayClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onCancel();
  };

  return createPortal(
    <div className={styles.overlay} onMouseDown={handleOverlayClick}>
      <div className={styles.dialog} role="dialog" aria-labelledby="picker-title">
        <h3 id="picker-title" className={styles.title}>Share your screen</h3>

        {loading && <div className={styles.loading}>Scanning monitors...</div>}

        {!loading && sources.length === 0 && (
          <div className={styles.loading}>No monitors found.</div>
        )}

        {!loading && sources.length > 0 && (
          <div className={styles.sourceGrid}>
            {sources.map((src) => (
              <button
                key={src.index}
                className={`${styles.sourceCard} ${selected === src.index ? styles.sourceCardSelected : ""}`}
                onClick={() => setSelected(src.index)}
                onDoubleClick={() => onSelect(src.index)}
                type="button"
              >
                {src.thumbnail ? (
                  <img className={styles.thumbnail} src={src.thumbnail} alt={src.name} />
                ) : (
                  <div className={styles.thumbnailPlaceholder}>No preview</div>
                )}
                <div className={styles.sourceInfo}>
                  <span className={styles.sourceName}>
                    {src.name || `Monitor ${src.index + 1}`}
                    {src.is_primary && <span className={styles.primaryBadge}>Primary</span>}
                  </span>
                  <span className={styles.sourceRes}>{src.width} x {src.height}</span>
                </div>
              </button>
            ))}
          </div>
        )}

        <div className={styles.actions}>
          <button className={styles.cancelBtn} onClick={onCancel} type="button">
            Cancel
          </button>
          <button
            className={styles.shareBtn}
            onClick={() => selected !== null && onSelect(selected)}
            disabled={selected === null}
            type="button"
          >
            Share
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
