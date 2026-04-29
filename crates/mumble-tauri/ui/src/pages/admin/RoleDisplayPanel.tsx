import { useMemo, useState } from "react";
import type { AclGroup } from "../../types";
import { RoleColorPicker } from "../../components/elements/RoleColorPicker";
import { RoleIconPicker } from "../../components/elements/RoleIconPicker";
import { RolePreviewCard } from "../../components/elements/RolePreviewCard";
import styles from "./AdminPanel.module.css";

const STYLE_PRESETS = [
  { id: "", label: "Default" },
  { id: "neon", label: "Neon outline" },
  { id: "gradient", label: "Gradient banner" },
  { id: "minimal", label: "Minimal" },
];

export interface RoleDisplayPanelProps {
  readonly role: AclGroup;
  readonly onPatch: (patch: Partial<AclGroup>) => void;
  readonly disabled?: boolean;
}

/** Display sub-tab of the role editor: name, color, icon, style preset, metadata. */
export function RoleDisplayPanel({ role, onPatch, disabled }: RoleDisplayPanelProps) {
  const metadataEntries = useMemo(
    () => Object.entries(role.metadata ?? {}),
    [role.metadata],
  );
  const [newKey, setNewKey] = useState("");
  const [newValue, setNewValue] = useState("");

  const setMetadata = (key: string, value: string | null) => {
    const next: Record<string, string> = { ...(role.metadata ?? {}) };
    if (value === null) {
      delete next[key];
    } else {
      next[key] = value;
    }
    onPatch({ metadata: next });
  };

  return (
    <div className={styles.editorGrid}>
      <div className={styles.editorMain}>
        <label className={styles.fieldLabel}>
          Role name
          <input
            type="text"
            className={styles.input}
            value={role.name}
            onChange={(e) => onPatch({ name: e.target.value })}
            disabled={disabled || role.inherited}
          />
        </label>

        <fieldset className={styles.fieldset}>
          <legend>Color</legend>
          <RoleColorPicker
            value={role.color}
            onChange={(color) => onPatch({ color })}
            disabled={disabled || role.inherited}
          />
        </fieldset>

        <fieldset className={styles.fieldset}>
          <legend>Icon</legend>
          <RoleIconPicker
            value={role.icon}
            onChange={(icon) => onPatch({ icon })}
            disabled={disabled || role.inherited}
          />
        </fieldset>

        <label className={styles.fieldLabel}>
          Style preset
          <select
            className={styles.select}
            value={role.style_preset ?? ""}
            onChange={(e) => onPatch({ style_preset: e.target.value || null })}
            disabled={disabled || role.inherited}
          >
            {STYLE_PRESETS.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </select>
        </label>

        <fieldset className={styles.fieldset}>
          <legend>Metadata</legend>
          {metadataEntries.length === 0 && (
            <span className={styles.dimText}>No metadata entries.</span>
          )}
          <ul className={styles.metadataList}>
            {metadataEntries.map(([k, v]) => (
              <li key={k} className={styles.metadataRow}>
                <span className={styles.metadataKey}>{k}</span>
                <input
                  type="text"
                  className={styles.input}
                  value={v}
                  onChange={(e) => setMetadata(k, e.target.value)}
                  disabled={disabled || role.inherited}
                />
                {!disabled && !role.inherited && (
                  <button
                    type="button"
                    className={styles.removeSmallBtn}
                    onClick={() => setMetadata(k, null)}
                    aria-label={`Remove ${k}`}
                  >
                    &times;
                  </button>
                )}
              </li>
            ))}
          </ul>
          {!disabled && !role.inherited && (
            <div className={styles.metadataAddRow}>
              <input
                type="text"
                className={styles.input}
                placeholder="key"
                value={newKey}
                onChange={(e) => setNewKey(e.target.value)}
              />
              <input
                type="text"
                className={styles.input}
                placeholder="value"
                value={newValue}
                onChange={(e) => setNewValue(e.target.value)}
              />
              <button
                type="button"
                className={styles.addBtn}
                onClick={() => {
                  const k = newKey.trim();
                  if (!k) return;
                  setMetadata(k, newValue);
                  setNewKey("");
                  setNewValue("");
                }}
              >
                Add
              </button>
            </div>
          )}
        </fieldset>
      </div>

      <aside className={styles.editorAside}>
        <RolePreviewCard
          name={role.name}
          color={role.color}
          icon={role.icon}
        />
      </aside>
    </div>
  );
}
