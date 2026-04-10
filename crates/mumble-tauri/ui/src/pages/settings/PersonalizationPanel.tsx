import { useRef, useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PersonalizationData, BubbleStyle, FontSize, BgFit, ChannelViewerStyle } from "../../personalizationStorage";
import { ImageEditor } from "./ImageEditor";
import { SliderField, Toggle } from "./SharedControls";
import styles from "./SettingsPage.module.css";

interface PersonalizationPanelProps {
  readonly data: PersonalizationData;
  readonly onChange: (patch: Partial<PersonalizationData>) => void;
  readonly isExpert: boolean;
}

/** Maximum dimension for the stored background (keep data-URL manageable). */
const MAX_BG_WIDTH = 1920;
const MAX_BG_HEIGHT = 1080;

const BUBBLE_STYLES: { id: BubbleStyle; label: string; icon: string }[] = [
  { id: "bubbles", label: "Bubbles", icon: "💬" },
  { id: "flat", label: "Flat", icon: "📋" },
  { id: "compact", label: "Compact", icon: "📟" },
];

const BG_FIT_OPTIONS: { id: BgFit; label: string; icon: string }[] = [
  { id: "cover", label: "Cover", icon: "🖼️" },
  { id: "tile", label: "Tile", icon: "🧩" },
];

const CHANNEL_VIEWER_STYLES: { id: ChannelViewerStyle; label: string; icon: string }[] = [
  { id: "classic", label: "Classic", icon: "🏛️" },
  { id: "flat", label: "Flat", icon: "📋" },
  { id: "modern", label: "Modern", icon: "✨" },
];

const FONT_SIZES: { id: FontSize; label: string }[] = [
  { id: "small", label: "Small" },
  { id: "medium", label: "Medium" },
  { id: "large", label: "Large" },
];

const FONT_FAMILIES: { id: string; label: string; css: string }[] = [
  { id: "system", label: "System Default", css: "inherit" },
  { id: "monospace", label: "Monospace", css: "'Cascadia Mono', 'Fira Code', 'Consolas', monospace" },
  { id: "serif", label: "Serif", css: "'Georgia', 'Times New Roman', serif" },
  { id: "humanist", label: "Humanist", css: "'Segoe UI', 'Helvetica Neue', 'Arial', sans-serif" },
  { id: "rounded", label: "Rounded", css: "'Nunito', 'Quicksand', 'Comfortaa', sans-serif" },
];

/**
 * Extract the raw base64 string from a data-URL.
 * E.g. `data:image/jpeg;base64,/9j/4AAQ...` -> `/9j/4AAQ...`
 */
function dataUrlToBase64(dataUrl: string): string {
  return dataUrl.split(",")[1];
}

/** Wrap a base64 string as a JPEG data-URL. */
function base64ToDataUrl(base64: string): string {
  return `data:image/jpeg;base64,${base64}`;
}

/** Debounce delay for the blur slider (ms). */
const BLUR_DEBOUNCE_MS = 500;

export function PersonalizationPanel({ data, onChange, isExpert }: PersonalizationPanelProps) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [editorImage, setEditorImage] = useState<string | null>(null);
  const [blurring, setBlurring] = useState(false);

  /** Monotonic counter to discard stale processing results. */
  const processGenRef = useRef(0);
  /** Debounce timer for blur/dim slider changes. */
  const processTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Clean up debounce timer on unmount.
  useEffect(() => {
    return () => {
      if (processTimerRef.current) clearTimeout(processTimerRef.current);
    };
  }, []);

  const hasBackground = Boolean(data.chatBgOriginal);
  const blurEnabled = data.chatBgBlurSigma > 0;

  /** Fire the backend `process_background` command (blur + dim) and store
   *  the result.  Returns immediately; the caller should have already bumped
   *  `processGenRef` and set `setBlurring(true)`. */
  const runProcessing = useCallback(
    (original: string, sigma: number, dim: number, gen: number) => {
      const imageBase64 = dataUrlToBase64(original);
      invoke<string>("process_background", { imageBase64, sigma, dim })
        .then((processed) => {
          if (processGenRef.current === gen) {
            onChange({ chatBgBlurred: base64ToDataUrl(processed) });
          }
        })
        .catch((e) => console.error("Background processing failed:", e))
        .finally(() => {
          if (processGenRef.current === gen) setBlurring(false);
        });
    },
    [onChange],
  );

  /** Schedule a debounced reprocess of the background image. */
  const scheduleProcessing = useCallback(
    (original: string, sigma: number, dim: number) => {
      if (processTimerRef.current) clearTimeout(processTimerRef.current);

      // If neither blur nor dim is active, clear the processed image.
      if (sigma <= 0 && dim <= 0) {
        processGenRef.current++;
        setBlurring(false);
        onChange({ chatBgBlurred: null });
        return;
      }

      processTimerRef.current = setTimeout(() => {
        const gen = ++processGenRef.current;
        setBlurring(true);
        runProcessing(original, sigma, dim, gen);
      }, BLUR_DEBOUNCE_MS);
    },
    [onChange, runProcessing],
  );

  // Pick an image file
  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => setEditorImage(reader.result as string);
    reader.readAsDataURL(file);
    e.target.value = "";
  };

  // After crop/resize in ImageEditor, store the original and reprocess.
  const handleEditorConfirm = useCallback(
    (dataUrl: string) => {
      setEditorImage(null);
      onChange({ chatBgOriginal: dataUrl, chatBgBlurred: null });

      const needsProcessing = data.chatBgBlurSigma > 0 || data.chatBgDim > 0;
      if (needsProcessing) {
        const gen = ++processGenRef.current;
        setBlurring(true);
        runProcessing(dataUrl, data.chatBgBlurSigma, data.chatBgDim, gen);
      }
    },
    [data.chatBgBlurSigma, data.chatBgDim, onChange, runProcessing],
  );

  // Remove the background (also invalidate any in-flight processing).
  const handleRemove = useCallback(() => {
    processGenRef.current++;
    if (processTimerRef.current) clearTimeout(processTimerRef.current);
    setBlurring(false);
    onChange({
      chatBgOriginal: null,
      chatBgBlurred: null,
      chatBgBlurSigma: 0,
    });
  }, [onChange]);

  // Toggle blur on/off (non-blocking).
  const handleToggleBlur = useCallback(() => {
    if (blurEnabled) {
      processGenRef.current++;
      if (processTimerRef.current) clearTimeout(processTimerRef.current);
      setBlurring(false);
      onChange({ chatBgBlurSigma: 0, chatBgBlurred: null });

      // Re-process with dim only if needed.
      if (data.chatBgOriginal && data.chatBgDim > 0) {
        const gen = ++processGenRef.current;
        setBlurring(true);
        runProcessing(data.chatBgOriginal, 0, data.chatBgDim, gen);
      }
    } else {
      const sigma = 8;
      onChange({ chatBgBlurSigma: sigma });

      if (data.chatBgOriginal) {
        const gen = ++processGenRef.current;
        setBlurring(true);
        runProcessing(data.chatBgOriginal, sigma, data.chatBgDim, gen);
      }
    }
  }, [blurEnabled, data.chatBgOriginal, data.chatBgDim, onChange, runProcessing]);

  // Change blur sigma -- debounced.
  const handleBlurSigmaChange = useCallback(
    (sigma: number) => {
      onChange({ chatBgBlurSigma: sigma });
      if (!data.chatBgOriginal) return;
      scheduleProcessing(data.chatBgOriginal, sigma, data.chatBgDim);
    },
    [data.chatBgOriginal, data.chatBgDim, onChange, scheduleProcessing],
  );

  // Change dim -- debounced.
  const handleDimChange = useCallback(
    (dim: number) => {
      onChange({ chatBgDim: dim });
      if (!data.chatBgOriginal) return;
      scheduleProcessing(data.chatBgOriginal, data.chatBgBlurSigma, dim);
    },
    [data.chatBgOriginal, data.chatBgBlurSigma, onChange, scheduleProcessing],
  );

  // The image to show in the preview (blurred if available, otherwise original)
  const previewImage = data.chatBgBlurred ?? data.chatBgOriginal;

  return (
    <>
      <h2 className={styles.panelTitle}>Personalize</h2>

      {/* -- Chat Background --------------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Chat Background</h3>
        <p className={styles.fieldHint}>
          Set a custom background image for your chat view.
        </p>

        {/* Preview */}
        {hasBackground && previewImage && (
          <div className={styles.bgPreview}>
            <img
              src={previewImage}
              alt="Chat background preview"
              className={styles.bgPreviewImg}
              style={{ opacity: data.chatBgOpacity }}
            />
            {blurring && (
              <div className={styles.bgPreviewOverlay}>Processing...</div>
            )}
          </div>
        )}

        {/* Upload / Remove buttons */}
        <input
          ref={fileInputRef}
          type="file"
          accept="image/png,image/jpeg,image/webp"
          style={{ display: "none" }}
          onChange={handleFileChange}
        />

        <div className={styles.avatarActions}>
          <button
            type="button"
            className={styles.ghostBtn}
            onClick={() => fileInputRef.current?.click()}
          >
            {hasBackground ? "Change Image" : "Choose Image"}
          </button>
          {hasBackground && (
            <button
              type="button"
              className={styles.ghostBtn}
              onClick={handleRemove}
            >
              Remove
            </button>
          )}
        </div>
      </section>

      {/* Blur & Appearance */}
      {hasBackground && (
        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>Background Effects</h3>

          {/* Blur toggle -- always visible */}
          <div className={styles.fieldRow}>
            <label className={styles.fieldLabel}>Blur Background</label>
            <Toggle checked={blurEnabled} onChange={handleToggleBlur} disabled={blurring} />
          </div>

          {/* Fit mode selector -- always visible */}
          <div className={styles.fieldRow}>
            <label className={styles.fieldLabel}>Image Fit</label>
          </div>
          <div className={styles.optionGrid}>
            {BG_FIT_OPTIONS.map((opt) => (
              <button
                key={opt.id}
                type="button"
                className={`${styles.optionCard} ${data.chatBgFit === opt.id ? styles.optionCardSelected : ""}`}
                onClick={() => onChange({ chatBgFit: opt.id })}
              >
                <span className={styles.optionPreview}>{opt.icon}</span>
                <span className={styles.optionLabel}>{opt.label}</span>
              </button>
            ))}
          </div>

          {/* Advanced options — only shown in expert/developer mode */}
          {isExpert && (
            <>
              {/* Blur strength slider */}
              {blurEnabled && (
                <SliderField
                  label="Blur Strength"
                  hint="Higher values produce a stronger blur."
                  min={1}
                  max={30}
                  step={1}
                  value={data.chatBgBlurSigma}
                  onChange={handleBlurSigmaChange}
                  format={(v) => `${v}`}
                />
              )}

              {/* Opacity slider */}
              <SliderField
                label="Image Opacity"
                hint="How visible the background image is."
                min={0.05}
                max={1}
                step={0.05}
                value={data.chatBgOpacity}
                onChange={(v) => onChange({ chatBgOpacity: v })}
                format={(v) => `${Math.round(v * 100)}%`}
              />

              {/* Dim overlay slider */}
              <SliderField
                label="Dim Overlay"
                hint="Darkens the background to improve text readability."
                min={0}
                max={0.9}
                step={0.05}
                value={data.chatBgDim}
                onChange={handleDimChange}
                format={(v) => `${Math.round(v * 100)}%`}
              />
            </>
          )}
        </section>
      )}

      {/* -- Message Style ----------------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Message Style</h3>
        <p className={styles.fieldHint}>
          Choose how chat messages are displayed.
        </p>
        <div className={styles.optionGrid}>
          {BUBBLE_STYLES.map((s) => (
            <button
              key={s.id}
              type="button"
              className={`${styles.optionCard} ${data.bubbleStyle === s.id ? styles.optionCardSelected : ""}`}
              onClick={() => onChange({ bubbleStyle: s.id })}
            >
              <span className={styles.optionPreview}>{s.icon}</span>
              <span className={styles.optionLabel}>{s.label}</span>
            </button>
          ))}
        </div>
      </section>

      {/* -- Font -------------------------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Font</h3>

        {/* Font size */}
        <div className={styles.field}>
          <label className={styles.fieldLabel}>Font Size</label>
          <div className={styles.optionGrid}>
            {FONT_SIZES.map((fs) => (
              <button
                key={fs.id}
                type="button"
                className={`${styles.optionCard} ${data.fontSize === fs.id ? styles.optionCardSelected : ""}`}
                onClick={() => onChange({ fontSize: fs.id })}
              >
                <span className={styles.optionLabel}>{fs.label}</span>
              </button>
            ))}
          </div>
        </div>

        {/* Custom px (expert only) */}
        {isExpert && (
          <SliderField
            label="Custom Font Size"
            hint="Override font size with an exact pixel value."
            min={10}
            max={24}
            step={1}
            value={data.fontSizeCustomPx}
            onChange={(v) => onChange({ fontSizeCustomPx: v, fontSize: "large" })}
            format={(v) => `${v}px`}
          />
        )}

        {/* Font family */}
        <div className={styles.field}>
          <label className={styles.fieldLabel}>Font Family</label>
          <div className={styles.optionGrid}>
            {FONT_FAMILIES.map((f) => (
              <button
                key={f.id}
                type="button"
                className={`${styles.optionCard} ${data.fontFamily === f.id ? styles.optionCardSelected : ""}`}
                style={{ fontFamily: f.css }}
                onClick={() => onChange({ fontFamily: f.id })}
              >
                <span className={styles.optionLabel}>{f.label}</span>
              </button>
            ))}
          </div>
        </div>
      </section>

      {/* -- Message List ------------------------------------------ */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Message List</h3>
        <div className={styles.fieldRow}>
          <div>
            <label className={styles.fieldLabel}>Compact Mode</label>
            <p className={styles.fieldHint}>
              Hide avatars and tighten spacing for higher density.
            </p>
          </div>
          <Toggle
            checked={data.compactMode}
            onChange={() => onChange({ compactMode: !data.compactMode })}
          />
        </div>
      </section>

      {/* -- Channel Viewer ---------------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Channel Viewer</h3>
        <p className={styles.fieldHint}>
          Choose how the channel list is displayed in the sidebar.
        </p>
        <div className={styles.optionGrid}>
          {CHANNEL_VIEWER_STYLES.map((s) => (
            <button
              key={s.id}
              type="button"
              className={`${styles.optionCard} ${data.channelViewerStyle === s.id ? styles.optionCardSelected : ""}`}
              onClick={() => onChange({ channelViewerStyle: s.id })}
            >
              <span className={styles.optionPreview}>{s.icon}</span>
              <span className={styles.optionLabel}>{s.label}</span>
            </button>
          ))}
        </div>
      </section>

      {/* -- Image editor overlay ---------------------------------- */}
      {editorImage && (
        <ImageEditor
          src={editorImage}
          cropShape="rect"
          targetWidth={MAX_BG_WIDTH}
          targetHeight={MAX_BG_HEIGHT}
          maxBytes={800_000}
          onConfirm={handleEditorConfirm}
          onCancel={() => setEditorImage(null)}
        />
      )}
    </>
  );
}
