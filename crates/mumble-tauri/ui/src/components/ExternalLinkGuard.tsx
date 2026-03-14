/**
 * ExternalLinkGuard
 *
 * Wraps any content that may contain sanitized bio HTML with external links
 * (marked data-external="true" by bioSanitize).  Click events on those links
 * are intercepted and a warning dialog is shown before the browser is asked to
 * open the URL.
 *
 * Usage:
 *   <ExternalLinkGuard className={styles.bioContent}>
 *     <div dangerouslySetInnerHTML={{ __html: cleanBio }} />
 *   </ExternalLinkGuard>
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import styles from "./ExternalLinkGuard.module.css";

// ─── Warning dialog ───────────────────────────────────────────────

interface DialogProps {
  url: string;
  onConfirm: () => void;
  onCancel: () => void;
}

function ExternalLinkDialog({ url, onConfirm, onCancel }: Readonly<DialogProps>) {
  const dialogRef = useRef<HTMLDialogElement>(null);

  // Open as a modal and attach backdrop-click + Escape handling via native
  // DOM listeners so no JSX event props are needed on the dialog element.
  useEffect(() => {
    const el = dialogRef.current;
    if (!el) return;
    el.showModal();

    // Dismiss when the user clicks the backdrop (coords outside dialog box).
    const handleClick = (e: MouseEvent) => {
      const rect = el.getBoundingClientRect();
      if (
        e.clientX < rect.left ||
        e.clientX > rect.right ||
        e.clientY < rect.top ||
        e.clientY > rect.bottom
      ) {
        onCancel();
      }
    };

    // Dismiss on the native Escape key (dialog fires a "cancel" event).
    const handleCancel = (e: Event) => {
      e.preventDefault();
      onCancel();
    };

    el.addEventListener("click", handleClick);
    el.addEventListener("cancel", handleCancel);
    return () => {
      el.removeEventListener("click", handleClick);
      el.removeEventListener("cancel", handleCancel);
    };
  }, [onCancel]);

  // Safely display the URL without trusting it.
  const displayUrl = (() => {
    try {
      const parsed = new URL(url);
      const full = parsed.hostname + parsed.pathname + parsed.search;
      return full.length > 60 ? full.slice(0, 57) + "..." : full;
    } catch {
      return url.length > 60 ? url.slice(0, 57) + "..." : url;
    }
  })();

  return (
    <dialog
      ref={dialogRef}
      className={styles.dialog}
      aria-labelledby="ext-link-title"
    >
      {/* Icon */}
      <div className={styles.iconRow}>
        <svg
          className={styles.warningIcon}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
          <line x1="12" y1="9" x2="12" y2="13" />
          <line x1="12" y1="17" x2="12.01" y2="17" />
        </svg>
      </div>

      <h2 id="ext-link-title" className={styles.title}>
        External Link
      </h2>

      <p className={styles.body}>
        You are about to leave <strong>Fancy Mumble</strong> and visit an
        external website:
      </p>

      <div className={styles.urlBox} title={url}>
        {displayUrl}
      </div>

      <p className={styles.disclaimer}>
        This website is not affiliated with or endorsed by Fancy Mumble.
        External links may lead to sites that contain harmful, misleading, or
        unwanted content. Proceed only if you trust the source.
      </p>

      <div className={styles.actions}>
        <button className={styles.cancelBtn} onClick={onCancel}>
          Cancel
        </button>
        <button className={styles.openBtn} onClick={onConfirm}>
          Open Link
        </button>
      </div>
    </dialog>
  );
}

// ─── Guard wrapper ────────────────────────────────────────────────

interface GuardProps {
  children: React.ReactNode;
  className?: string;
}

/**
 * Intercepts clicks on anchors tagged with data-external="true" inside its
 * subtree and shows a confirmation dialog before opening the URL.
 *
 * Uses a native DOM event listener (not a JSX onClick prop) so the wrapper
 * div does not need to be a focusable / interactive element.
 */
export function ExternalLinkGuard({ children, className }: Readonly<GuardProps>) {
  const [pendingUrl, setPendingUrl] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Attach a native click listener so the wrapper div is not declared
  // as an interactive element in JSX (avoids a11y lint violations).
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const handler = (e: MouseEvent) => {
      const anchor = (e.target as Element).closest("a[data-external]");
      if (!anchor) return;
      e.preventDefault();
      e.stopPropagation();
      const href = anchor.getAttribute("href");
      if (href) setPendingUrl(href);
    };

    el.addEventListener("click", handler);
    return () => el.removeEventListener("click", handler);
  }, []);

  const handleConfirm = useCallback(() => {
    if (pendingUrl) {
      openUrl(pendingUrl).catch(() => {
        // Fallback for non-Tauri environments (e.g. Vite dev server).
        window.open(pendingUrl, "_blank", "noopener,noreferrer");
      });
    }
    setPendingUrl(null);
  }, [pendingUrl]);

  const handleCancel = useCallback(() => {
    setPendingUrl(null);
  }, []);

  return (
    <>
      <div ref={containerRef} className={className}>
        {children}
      </div>
      {pendingUrl && (
        <ExternalLinkDialog
          url={pendingUrl}
          onConfirm={handleConfirm}
          onCancel={handleCancel}
        />
      )}
    </>
  );
}
