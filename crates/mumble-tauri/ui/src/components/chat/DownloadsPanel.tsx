import { useCallback } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { useAppStore } from "../../store";
import type { DownloadEntry } from "../../types";
import {
  formatBytes,
  previewKindForFilename,
  type PreviewKind,
} from "./FileAttachmentCard";
import CloseIcon from "../../assets/icons/action/close.svg?react";
import styles from "./DownloadsPanel.module.css";

interface DownloadsPanelProps {
  readonly onClose: () => void;
}

function formatRelativeTime(ts: number): string {
  const diff = Date.now() - ts;
  const sec = Math.round(diff / 1000);
  if (sec < 60) return "just now";
  const min = Math.round(sec / 60);
  if (min < 60) return `${min} min ago`;
  const hr = Math.round(min / 60);
  if (hr < 24) return `${hr} h ago`;
  return new Date(ts).toLocaleString();
}

function PreviewMedia({ entry, kind }: { entry: DownloadEntry; kind: PreviewKind }) {
  const src = convertFileSrc(entry.destPath);
  if (kind === "image") {
    return <img src={src} alt={entry.filename} className={styles.previewImage} loading="lazy" />;
  }
  if (kind === "audio") {
    return (
      <audio controls preload="none" src={src} className={styles.previewAudio}>
        <track kind="captions" />
      </audio>
    );
  }
  if (kind === "video") {
    return (
      <video controls preload="metadata" src={src} className={styles.previewVideo}>
        <track kind="captions" />
      </video>
    );
  }
  return null;
}

function FileTypeIcon({ kind }: { kind: PreviewKind }) {
  // Compact emoji-based glyphs to avoid pulling in extra SVGs.
  const map: Record<PreviewKind, string> = {
    image: "\u{1F5BC}",  // framed picture
    audio: "\u{1F3B5}",  // musical note
    video: "\u{1F3AC}",  // clapper board
    text:  "\u{1F4C4}",  // page facing up
    other: "\u{1F4E6}",  // package
  };
  return <span className={styles.typeIcon} aria-hidden="true">{map[kind]}</span>;
}

export default function DownloadsPanel({ onClose }: DownloadsPanelProps) {
  const downloads = useAppStore((s) => s.downloads);
  const removeDownload = useAppStore((s) => s.removeDownload);
  const clearDownloads = useAppStore((s) => s.clearDownloads);

  const handleOpen = useCallback(async (entry: DownloadEntry) => {
    try {
      await openPath(entry.destPath);
    } catch (e) {
      console.error("openPath failed:", e);
    }
  }, []);

  const handleReveal = useCallback(async (entry: DownloadEntry) => {
    try {
      await revealItemInDir(entry.destPath);
    } catch (e) {
      console.error("revealItemInDir failed:", e);
    }
  }, []);

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <span className={styles.title}>
          Downloads
          {downloads.length > 0 && (
            <span className={styles.count}>{downloads.length}</span>
          )}
        </span>
        <div className={styles.headerActions}>
          {downloads.length > 0 && (
            <button
              type="button"
              className={styles.clearBtn}
              onClick={clearDownloads}
              title="Clear list (does not delete files)"
            >
              Clear
            </button>
          )}
          <button
            type="button"
            className={styles.closeBtn}
            onClick={onClose}
            aria-label="Close downloads"
          >
            <CloseIcon width={16} height={16} />
          </button>
        </div>
      </div>

      {downloads.length === 0 ? (
        <div className={styles.empty}>No files downloaded yet.</div>
      ) : (
        <div className={styles.list}>
          {downloads.map((entry) => {
            const kind = previewKindForFilename(entry.filename);
            const hasInlinePreview = kind === "image" || kind === "audio" || kind === "video";
            return (
              <div key={entry.id} className={styles.item}>
                <div className={styles.itemHeader}>
                  {!hasInlinePreview && <FileTypeIcon kind={kind} />}
                  <button
                    type="button"
                    className={styles.filename}
                    onClick={() => handleOpen(entry)}
                    title={entry.destPath}
                  >
                    {entry.filename}
                  </button>
                  <span className={styles.timeAgo}>{formatRelativeTime(entry.downloadedAt)}</span>
                </div>

                {hasInlinePreview && (
                  <div className={styles.previewWrap}>
                    <PreviewMedia entry={entry} kind={kind} />
                  </div>
                )}

                <div className={styles.meta}>
                  <span>{formatBytes(entry.sizeBytes)}</span>
                  <span className={styles.path} title={entry.destPath}>{entry.destPath}</span>
                </div>

                <div className={styles.actions}>
                  <button type="button" className={styles.actionBtn} onClick={() => handleOpen(entry)}>
                    Open
                  </button>
                  <button type="button" className={styles.actionBtn} onClick={() => handleReveal(entry)}>
                    Show in folder
                  </button>
                  <button
                    type="button"
                    className={`${styles.actionBtn} ${styles.removeBtn}`}
                    onClick={() => removeDownload(entry.id)}
                    title="Remove from list (does not delete file)"
                  >
                    Remove
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
