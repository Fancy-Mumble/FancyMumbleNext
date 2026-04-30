import { useState, useEffect, useCallback, useRef } from "react";
import type { FancyProfile } from "../../types";
import { KlipyGifBrowser } from "./KlipyGifBrowser";
import { ImageEditor } from "./ImageEditor";
import { FileDropZone } from "../../components/elements/FileDropZone";
import styles from "./BannerEditorModal.module.css";
import settingsStyles from "./SettingsPage.module.css";

type BannerTab = "color" | "image" | "gif";

function isKlipyUrl(url: string | undefined): boolean {
  return !!url && url.includes("klipy.com");
}

function detectInitialTab(banner: FancyProfile["banner"]): BannerTab {
  if (banner?.image) {
    return isKlipyUrl(banner.image) ? "gif" : "image";
  }
  return "color";
}

interface BannerEditorModalProps {
  banner: FancyProfile["banner"];
  onConfirm: (banner: FancyProfile["banner"]) => void;
  onCancel: () => void;
}

export function BannerEditorModal({
  banner,
  onConfirm,
  onCancel,
}: Readonly<BannerEditorModalProps>) {
  const initialTab = detectInitialTab(banner);
  const [tab, setTab] = useState<BannerTab>(initialTab);
  const [color, setColor] = useState(banner?.color || "#1a1a2e");

  const [localImage, setLocalImage] = useState<string | undefined>(
    initialTab === "image" ? banner?.image : undefined,
  );
  const [klipyGif, setKlipyGif] = useState<string | undefined>(
    initialTab === "gif" ? banner?.image : undefined,
  );

  const [editorImage, setEditorImage] = useState<string | null>(null);
  const [showUnsavedHint, setShowUnsavedHint] = useState(false);
  const flashTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  const activeImage = tab === "gif" ? klipyGif : tab === "image" ? localImage : undefined;

  const hasChanges = useCallback(() => {
    const origColor = banner?.color || "#1a1a2e";
    const origImage = banner?.image;
    return color !== origColor || activeImage !== origImage;
  }, [banner, color, activeImage]);

  const tryClose = useCallback(() => {
    if (!hasChanges()) {
      onCancel();
      return;
    }
    setShowUnsavedHint(true);
    clearTimeout(flashTimerRef.current);
    flashTimerRef.current = setTimeout(() => setShowUnsavedHint(false), 3000);
  }, [hasChanges, onCancel]);

  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") tryClose();
    };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [tryClose]);

  useEffect(() => () => clearTimeout(flashTimerRef.current), []);

  const handleFileSelect = useCallback((file: File) => {
    const reader = new FileReader();
    reader.onload = () => {
      const dataUrl = reader.result as string;
      if (file.type === "image/gif") {
        setLocalImage(dataUrl);
      } else {
        setEditorImage(dataUrl);
      }
    };
    reader.readAsDataURL(file);
  }, []);

  const handleEditorConfirm = useCallback((dataUrl: string) => {
    setLocalImage(dataUrl);
    setEditorImage(null);
  }, []);

  const handleGifSelect = useCallback((url: string) => {
    setKlipyGif(url);
  }, []);

  const handleApply = () => {
    onConfirm({ color, image: activeImage });
  };

  const handleRemoveImage = () => {
    setLocalImage(undefined);
  };

  return (
    <div className={settingsStyles.editorOverlay} onClick={tryClose}>
      <div
        className={styles.modal}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className={styles.title}>Edit Banner</h3>

        {/* Preview */}
        <div
          className={styles.preview}
          style={{
            backgroundColor: color,
            backgroundImage: activeImage ? `url(${activeImage})` : undefined,
            backgroundSize: "cover",
            backgroundPosition: "center",
          }}
        />

        {/* Tabs */}
        <div className={styles.tabs}>
          <button
            type="button"
            className={`${styles.tab} ${tab === "color" ? styles.tabActive : ""}`}
            onClick={() => setTab("color")}
          >
            Solid Colour
          </button>
          <button
            type="button"
            className={`${styles.tab} ${tab === "image" ? styles.tabActive : ""}`}
            onClick={() => setTab("image")}
          >
            Image / GIF
          </button>
          <button
            type="button"
            className={`${styles.tab} ${tab === "gif" ? styles.tabActive : ""}`}
            onClick={() => setTab("gif")}
          >
            Klipy GIF
          </button>
        </div>

        {/* Tab content */}
        <div className={styles.tabContent}>
          {tab === "color" && (
            <div className={styles.colorSection}>
              <label className={styles.label}>Banner colour</label>
              <input
                type="color"
                className={settingsStyles.colorInput}
                value={color}
                onChange={(e) => setColor(e.target.value)}
              />
            </div>
          )}

          {tab === "image" && (
            <FileDropZone
              accept="image/png,image/jpeg,image/webp,image/gif"
              onFile={handleFileSelect}
              label="Drop an image or GIF here, or click to browse"
              preview={
                localImage ? (
                  <img src={localImage} alt="Banner" />
                ) : undefined
              }
              onRemove={localImage ? handleRemoveImage : undefined}
            />
          )}

          {tab === "gif" && (
            <KlipyGifBrowser onSelect={handleGifSelect} />
          )}
        </div>

        {/* Actions */}
        <div className={styles.actions}>
          <button
            type="button"
            className={settingsStyles.ghostBtn}
            onClick={onCancel}
          >
            Discard
          </button>
          <div className={styles.applyWrapper}>
            {showUnsavedHint && (
              <div className={styles.unsavedBubble}>
                You have unsaved changes. Apply or discard them.
              </div>
            )}
            <button
              type="button"
              className={`${settingsStyles.applyBtn} ${showUnsavedHint ? styles.applyFlash : ""}`}
              onClick={handleApply}
            >
              Apply
            </button>
          </div>
        </div>
      </div>

      {editorImage && (
        <ImageEditor
          src={editorImage}
          cropShape="rect"
          targetWidth={400}
          targetHeight={150}
          maxBytes={80_000}
          onConfirm={handleEditorConfirm}
          onCancel={() => setEditorImage(null)}
        />
      )}
    </div>
  );
}
