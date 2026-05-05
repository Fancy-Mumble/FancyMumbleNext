import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { rebaseFileServerUrl, useAppStore } from "../../store";
import type { FileAccessMode } from "../../types";
import { MediaLightbox } from "./MediaPreview";
import styles from "./FileAttachmentCard.module.css";

export interface FileAttachmentInfo {
  /** Signed download URL returned by the file-server. */
  readonly url: string;
  /** Display filename (best-effort; may differ from the actual blob name). */
  readonly filename: string;
  /** File size in bytes (purely informational). */
  readonly sizeBytes?: number;
  /** Access mode used at upload time. */
  readonly mode: FileAccessMode;
  /** Unix-seconds expiry, or `null` if the file never expires. */
  readonly expiresAt?: number | null;
}

interface FileAttachmentCardProps {
  readonly info: FileAttachmentInfo;
}

/** HTML-comment marker used to embed a file attachment in a chat message
 *  body. Renderers detect the marker and render a {@link FileAttachmentCard}
 *  in place of the raw markdown link. Legacy clients see the inert comment. */
export const FANCY_FILE_MARKER_RE = /<!-- FANCY_FILE:([A-Za-z0-9+/=]+) -->/;

function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Serialise a {@link FileAttachmentInfo} to the FANCY_FILE marker comment. */
export function encodeFileAttachmentMarker(info: FileAttachmentInfo): string {
  const json = JSON.stringify(info);
  const b64 = bytesToBase64(new TextEncoder().encode(json));
  return `<!-- FANCY_FILE:${b64} -->`;
}

/** Parse a FANCY_FILE marker payload (the captured base64 group) into a
 *  {@link FileAttachmentInfo}, or `null` if it cannot be decoded. */
export function decodeFileAttachmentPayload(b64: string): FileAttachmentInfo | null {
  try {
    const json = new TextDecoder().decode(base64ToBytes(b64));
    const parsed = JSON.parse(json) as FileAttachmentInfo;
    if (typeof parsed?.url !== "string" || typeof parsed?.filename !== "string") {
      return null;
    }
    return { ...parsed, url: rebaseFileServerUrl(parsed.url) };
  } catch {
    return null;
  }
}

function formatBytes(bytes: number | undefined): string {
  if (bytes === undefined) return "";
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KiB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MiB`;
  return `${(mb / 1024).toFixed(2)} GiB`;
}

export { formatBytes };

/** Best-effort preview category for a filename, used by the Downloads panel. */
export type PreviewKind = "image" | "audio" | "video" | "text" | "other";

const IMAGE_EXTS = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "avif"]);
const AUDIO_EXTS = new Set(["mp3", "wav", "ogg", "flac", "m4a", "aac", "opus", "oga"]);
const VIDEO_EXTS = new Set(["mp4", "webm", "mov", "mkv", "m4v", "ogv"]);
const TEXT_EXTS = new Set(["txt", "md", "log", "json", "csv", "xml", "html", "css", "js", "ts", "rs", "py", "yml", "yaml", "toml"]);

export function previewKindForFilename(filename: string): PreviewKind {
  const dot = filename.lastIndexOf(".");
  if (dot < 0) return "other";
  const ext = filename.slice(dot + 1).toLowerCase();
  if (IMAGE_EXTS.has(ext)) return "image";
  if (AUDIO_EXTS.has(ext)) return "audio";
  if (VIDEO_EXTS.has(ext)) return "video";
  if (TEXT_EXTS.has(ext)) return "text";
  return "other";
}

export default function FileAttachmentCard({ info }: FileAttachmentCardProps) {
  const downloadFile = useAppStore((s) => s.downloadFile);
  const addDownload = useAppStore((s) => s.addDownload);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [lightboxOpen, setLightboxOpen] = useState(false);
  const initiallyExpired =
    info.expiresAt != null && info.expiresAt > 0 && info.expiresAt * 1000 < Date.now();
  const [expired, setExpired] = useState<boolean>(initiallyExpired);

  const kind = previewKindForFilename(info.filename);
  const previewable = kind === "image" || kind === "audio" || kind === "video";

  // Post-download: local asset URL (works for any access mode).
  // Pre-download: public files only - URL is a signed but open link.
  const previewSrc = savedPath
    ? convertFileSrc(savedPath)
    : (info.mode === "public" && previewable ? info.url : null);

  const handleOpenInBrowser = useCallback(() => {
    openUrl(info.url).catch(() => {
      // Fallback for non-Tauri environments (e.g. Vite dev server) or when
      // the opener plugin call fails for any reason.
      window.open(info.url, "_blank", "noopener,noreferrer");
    });
  }, [info.url]);

  const handleImageClick = useCallback(() => {
    if (previewSrc) setLightboxOpen(true);
  }, [previewSrc]);

  const closeLightbox = useCallback(() => setLightboxOpen(false), []);

  // Switch to expired state automatically once the announced expiry
  // timestamp passes, without waiting for a network failure.
  useEffect(() => {
    if (savedPath || expired) return;
    if (info.expiresAt == null || info.expiresAt <= 0) return;
    const msUntilExpiry = info.expiresAt * 1000 - Date.now();
    if (msUntilExpiry <= 0) {
      setExpired(true);
      return;
    }
    const timer = globalThis.setTimeout(() => setExpired(true), msUntilExpiry + 500);
    return () => globalThis.clearTimeout(timer);
  }, [info.expiresAt, savedPath, expired]);

  // Probe the URL when an inline preview fails to load. The file-server
  // returns HTTP 404 with a JSON body of `{"error":"link expired"}` for
  // expired signed URLs - distinguish that from a generic load failure.
  const probeForExpiry = useCallback(async () => {
    try {
      const resp = await fetch(info.url, { method: "GET" });
      if (resp.status === 404) {
        let body = "";
        try {
          body = await resp.text();
        } catch {
          // ignore body parse failure
        }
        if (body.toLowerCase().includes("expired")) {
          setExpired(true);
          return;
        }
      }
      setError("Preview failed to load.");
    } catch {
      setError("Preview failed to load.");
    }
  }, [info.url]);

  const handlePreviewError = useCallback(() => {
    if (expired) return;
    void probeForExpiry();
  }, [expired, probeForExpiry]);

  const canOpenInBrowser = (info.mode === "public" || info.mode === "password") && !expired;

  const onSave = useCallback(async () => {
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const dest = await save({ defaultPath: info.filename });
      if (!dest) {
        setBusy(false);
        return;
      }
      let password: string | undefined;
      if (info.mode === "password") {
        const entered = window.prompt("This file requires a password:");
        if (entered === null) {
          setBusy(false);
          return;
        }
        password = entered;
      }
      const written = await downloadFile({ url: info.url, destPath: dest, password });
      addDownload({
        filename: info.filename,
        destPath: dest,
        sizeBytes: written,
        sourceUrl: info.url,
        mode: info.mode,
      });
      setSaved(true);
      setSavedPath(dest);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.toLowerCase().includes("expired")) {
        setExpired(true);
      } else {
        setError(msg);
      }
    } finally {
      setBusy(false);
    }
  }, [downloadFile, addDownload, info]);

  const preview = (() => {
    if (!previewSrc) return null;
    if (kind === "image") {
      return (
        <button
          type="button"
          className={styles.previewImageBtn}
          onClick={handleImageClick}
          aria-label={`View ${info.filename} in lightbox`}
        >
          <img
            src={previewSrc}
            alt={info.filename}
            className={styles.previewImage}
            loading="lazy"
            onError={handlePreviewError}
          />
        </button>
      );
    }
    if (kind === "audio") {
      return (
        <div className={styles.previewAudioWrap}>
          <audio
            controls
            preload="none"
            src={previewSrc}
            className={styles.previewAudio}
            onError={handlePreviewError}
          >
            <track kind="captions" />
          </audio>
        </div>
      );
    }
    if (kind === "video") {
      return (
        <video
          controls
          preload="metadata"
          src={previewSrc}
          className={styles.previewVideo}
          onError={handlePreviewError}
        >
          <track kind="captions" />
        </video>
      );
    }
    return null;
  })();

  if (expired) {
    return (
      <div className={`${styles.card} ${styles.expiredCard}`}>
        <div className={styles.cardRow}>
          <div className={`${styles.icon} ${styles.expiredIcon}`} aria-hidden="true">
            <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                 strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="9" />
              <polyline points="12 7 12 12 15 14" />
            </svg>
          </div>
          <div className={styles.body}>
            <div className={styles.filename}>{info.filename}</div>
            <div className={styles.expiredMessage}>
              This download link has expired and is no longer available.
            </div>
            <div className={styles.meta}>
              {formatBytes(info.sizeBytes)}
              {info.mode !== "public" && <span className={styles.badge}>{info.mode}</span>}
              {info.expiresAt != null && info.expiresAt > 0 && (
                <span className={styles.expiry}>
                  expired {new Date(info.expiresAt * 1000).toLocaleString()}
                </span>
              )}
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.card}>
      {preview}
      <div className={styles.cardRow}>
        <div className={styles.icon} aria-hidden="true">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor"
               strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <polyline points="14 2 14 8 20 8" />
          </svg>
        </div>
        <div className={styles.body}>
          <div className={styles.filename}>{info.filename}</div>
          <div className={styles.meta}>
            {formatBytes(info.sizeBytes)}
            {info.mode !== "public" && <span className={styles.badge}>{info.mode}</span>}
            {info.expiresAt && (
              <span className={styles.expiry}>
                expires {new Date(info.expiresAt * 1000).toLocaleString()}
              </span>
            )}
          </div>
          {error && <div className={styles.error}>{error}</div>}
        </div>
        <button
          type="button"
          className={styles.saveBtn}
          onClick={onSave}
          disabled={busy}
          title={saved ? "Saved - download again" : "Download to disk"}
        >
          {busy ? "Saving\u2026" : saved ? "Saved" : "Save"}
        </button>
        {canOpenInBrowser && (
          <button
            type="button"
            className={styles.openBtn}
            onClick={handleOpenInBrowser}
            title="Open in browser"
          >
            Open
          </button>
        )}
      </div>
      {lightboxOpen && previewSrc && kind === "image" && createPortal(
        <MediaLightbox
          item={{ kind: "image", src: previewSrc, alt: info.filename }}
          onClose={closeLightbox}
        />,
        document.body,
      )}
    </div>
  );
}
