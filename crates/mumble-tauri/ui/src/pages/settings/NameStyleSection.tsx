import type { FancyProfile } from "../../types";
import { Toggle, SliderField } from "./SharedControls";
import { FONTS } from "./profileData";
import styles from "./SettingsPage.module.css";

type NameStyle = NonNullable<FancyProfile["nameStyle"]>;

export function NameStyleSection({
  nameStyle,
  onPatch,
  displayName,
}: Readonly<{
  nameStyle: NameStyle;
  onPatch: (patch: Partial<NameStyle>) => void;
  displayName: string;
}>) {
  const fontCss =
    FONTS.find((f) => f.id === (nameStyle.font ?? "default"))?.css ?? "inherit";

  return (
    <section className={styles.section}>
      <h3 className={styles.sectionTitle}>Name Style</h3>
      <p className={styles.fieldHint}>
        Customise how your name is rendered for FancyMumble users.
      </p>

      {/* Live preview */}
      <div
        className={styles.namePreview}
        style={{
          fontFamily: fontCss,
          color: nameStyle.gradient
            ? "transparent"
            : nameStyle.color || "var(--color-text-primary)",
          fontWeight: nameStyle.bold ? "bold" : "normal",
          fontStyle: nameStyle.italic ? "italic" : "normal",
          textShadow: nameStyle.glow
            ? `0 0 ${nameStyle.glow.size}px ${nameStyle.glow.color}`
            : "none",
          background: nameStyle.gradient
            ? `linear-gradient(135deg,${nameStyle.gradient[0]},${nameStyle.gradient[1]})`
            : "transparent",
          WebkitBackgroundClip: nameStyle.gradient ? "text" : undefined,
          WebkitTextFillColor: nameStyle.gradient ? "transparent" : undefined,
        }}
      >
        {displayName || "Your Name"}
      </div>

      {/* Font */}
      <div className={styles.field}>
        <label className={styles.fieldLabel}>Font</label>
        <div className={styles.optionGrid}>
          {FONTS.map((f) => (
            <button
              key={f.id}
              type="button"
              className={`${styles.optionCard} ${(nameStyle.font ?? "default") === f.id ? styles.optionCardSelected : ""}`}
              style={{ fontFamily: f.css }}
              onClick={() =>
                onPatch({
                  font: f.id === "default" ? undefined : f.id,
                })
              }
            >
              <span className={styles.optionLabel}>{f.label}</span>
            </button>
          ))}
        </div>
      </div>

      {/* Color */}
      <div className={styles.field}>
        <div className={styles.fieldRow}>
          <label className={styles.fieldLabel}>Text Colour</label>
          <input
            type="color"
            className={styles.colorInput}
            value={nameStyle.color || "#ffffff"}
            onChange={(e) => onPatch({ color: e.target.value })}
          />
        </div>
      </div>

      {/* Gradient */}
      <div className={styles.field}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <label className={styles.fieldLabel}>Gradient</label>
          </div>
          <Toggle
            checked={!!nameStyle.gradient}
            onChange={() =>
              onPatch({
                gradient: nameStyle.gradient
                  ? undefined
                  : ["#667eea", "#764ba2"],
              })
            }
          />
        </div>
        {nameStyle.gradient && (
          <div className={styles.gradientRow}>
            <input
              type="color"
              className={styles.colorInput}
              value={nameStyle.gradient[0]}
              onChange={(e) =>
                onPatch({
                  gradient: [e.target.value, nameStyle.gradient![1]],
                })
              }
            />
            <span className={styles.fieldLabel}>-&gt;</span>
            <input
              type="color"
              className={styles.colorInput}
              value={nameStyle.gradient[1]}
              onChange={(e) =>
                onPatch({
                  gradient: [nameStyle.gradient![0], e.target.value],
                })
              }
            />
          </div>
        )}
      </div>

      {/* Glow */}
      <div className={styles.field}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <label className={styles.fieldLabel}>Glow Effect</label>
          </div>
          <Toggle
            checked={!!nameStyle.glow}
            onChange={() =>
              onPatch({
                glow: nameStyle.glow
                  ? undefined
                  : { color: "#667eea", size: 6 },
              })
            }
          />
        </div>
        {nameStyle.glow && (
          <div className={styles.gradientRow}>
            <input
              type="color"
              className={styles.colorInput}
              value={nameStyle.glow.color}
              onChange={(e) =>
                onPatch({
                  glow: { ...nameStyle.glow!, color: e.target.value },
                })
              }
            />
            <SliderField
              label="Size"
              min={1}
              max={20}
              step={1}
              value={nameStyle.glow.size}
              onChange={(v) =>
                onPatch({
                  glow: { ...nameStyle.glow!, size: v },
                })
              }
              format={(v) => `${v}px`}
            />
          </div>
        )}
      </div>

      {/* Bold / Italic */}
      <div className={styles.field}>
        <div className={styles.toggleRow}>
          <label className={styles.fieldLabel}>Bold</label>
          <Toggle
            checked={!!nameStyle.bold}
            onChange={() => onPatch({ bold: !nameStyle.bold })}
          />
        </div>
      </div>
      <div className={styles.field}>
        <div className={styles.toggleRow}>
          <label className={styles.fieldLabel}>Italic</label>
          <Toggle
            checked={!!nameStyle.italic}
            onChange={() => onPatch({ italic: !nameStyle.italic })}
          />
        </div>
      </div>
    </section>
  );
}
