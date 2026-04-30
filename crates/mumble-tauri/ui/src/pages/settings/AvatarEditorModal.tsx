import { useState, useEffect, useCallback, useRef } from "react";
import { KlipyGifBrowser } from "./KlipyGifBrowser";
import { ImageEditor } from "./ImageEditor";
import { FileDropZone } from "../../components/elements/FileDropZone";
import { fetchAsDataUrl } from "../../utils/media";
import styles from "./BannerEditorModal.module.css";
import settingsStyles from "./SettingsPage.module.css";

type AvatarTab = "image" | "gif";

function isKlipyUrl(url: string | undefined): boolean {
  return !!url && url.includes("klipy.com");
}

function detectInitialTab(avatar: string | null): AvatarTab {
  if (avatar && isKlipyUrl(avatar)) return "gif";
  return "image";
}

interface AvatarEditorModalProps {
  avatar: string | null;
  onConfirm: (avatar: string | null) => void;
  onCancel: () => void;
}

export function AvatarEditorModal({
  avatar,
  onConfirm,
  onCancel,
}: Readonly<AvatarEditorModalProps>) {
  const initialTab = detectInitialTab(avatar);
  const [tab, setTab] = useState<AvatarTab>(initialTab);

  const [localImage, setLocalImage] = useState<string | undefined>(
    initialTab === "image" && avatar ? avatar : undefined,
  );
  const [klipyGif, setKlipyGif] = useState<string | undefined>(
    initialTab === "gif" && avatar ? avatar : undefined,
  );

  const [editorImage, setEditorImage] = useState<string | null>(null);
  const [showUnsavedHint, setShowUnsavedHint] = useState(false);
  const flashTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  const activeImage = tab === "gif" ? klipyGif : localImage;

  const hasChanges = useCallback(() => {
    return (activeImage ?? null) !== (avatar ?? null);
  }, [avatar, activeImage]);

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
    fetchAsDataUrl(url)
      .then((dataUrl) => setKlipyGif(dataUrl))
      .catch((err) => console.error("Failed to fetch Klipy GIF:", err));
  }, []);

  const handleApply = () => {
    onConfirm(activeImage ?? null);
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
        <h3 className={styles.title}>Edit Avatar</h3>

        {/* Preview */}
        <div className={styles.avatarPreview}>
          {activeImage ? (
            <img src={activeImage} alt="Avatar preview" />
          ) : (
            <span className={styles.avatarPlaceholder}>No image</span>
          )}
        </div>

        {/* Tabs */}
        <div className={styles.tabs}>
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
          {tab === "image" && (
            <FileDropZone
              accept="image/png,image/jpeg,image/webp,image/gif"
              onFile={handleFileSelect}
              label="Drop an image or GIF here, or click to browse"
              shape="circle"
              preview={
                localImage ? (
                  <img src={localImage} alt="Avatar" />
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
          cropShape="circle"
          targetWidth={128}
          targetHeight={128}
          maxBytes={100_000}
          onConfirm={handleEditorConfirm}
          onCancel={() => setEditorImage(null)}
        />
      )}
    </div>
  );
}
