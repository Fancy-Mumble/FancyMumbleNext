/**
 * Branded auto-updater bootstrapper window.
 *
 * Rendered when the React entry point detects the `?updater=1` flag
 * (see [`isUpdaterWindow`]). The window has its own minimal styling
 * and does not depend on the rest of the application's theming
 * infrastructure - this is intentional: the updater must keep working
 * even when the main app's chunks fail to load.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import BrandLogo from "../components/elements/BrandLogo";
import {
  isAutoInstall,
  updaterApi,
  type ProgressEvent,
  type UpdateInfo,
} from "./api";
import styles from "./UpdaterWindow.module.css";

enum Phase {
  Idle = "idle",
  Downloading = "downloading",
  Installing = "installing",
  Done = "done",
  Error = "error",
}

const APP_NAME = "Fancy Mumble";

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function getSubtitle(
  phase: Phase,
  info: UpdateInfo | null,
  autoInstall: boolean,
): string {
  if (phase !== Phase.Idle) {
    return `${APP_NAME} will restart automatically when finished.`;
  }
  if (!info) return "Checking for updates...";
  if (autoInstall) return `Preparing to install the latest version of ${APP_NAME}...`;
  return `A new version of ${APP_NAME} is ready to install.`;
}

function getProgressText(
  phase: Phase,
  percent: number | null,
): string {
  if (phase === Phase.Installing) return "Verifying & installing...";
  if (percent == null) return "Starting download...";
  return `${percent}%`;
}

function getHeading(phase: Phase): string {
  switch (phase) {
    case Phase.Downloading: return "Downloading update";
    case Phase.Installing:  return "Installing update";
    case Phase.Done:        return "Restarting...";
    case Phase.Error:       return "Update failed";
    default:                return "Update available";
  }
}

export default function UpdaterWindow() {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [phase, setPhase] = useState<Phase>(Phase.Idle);
  const [downloaded, setDownloaded] = useState(0);
  const [total, setTotal] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);
  const autoInstall = useMemo(() => isAutoInstall(), []);
  const autoStartedRef = useRef(false);
  const [skipVersion, setSkipVersion] = useState(false);

  const handleProgress = useCallback((event: ProgressEvent) => {
    if (event.kind === "started") {
      setPhase(Phase.Downloading);
      setTotal(event.total);
      setDownloaded(0);
    } else if (event.kind === "chunk") {
      setDownloaded(event.downloaded);
      if (event.total != null) setTotal(event.total);
    } else if (event.kind === "finished") {
      setPhase(Phase.Installing);
    }
  }, []);

  const onInstall = useCallback(async () => {
    setError(null);
    setPhase(Phase.Downloading);
    try {
      await updaterApi.install();
      setPhase(Phase.Done);
      if (import.meta.env.DEV) {
        console.warn("[updater] dev mode: skipping relaunch, dismissing window");
        setTimeout(() => {
          updaterApi.dismiss().catch(() => updaterApi.closeWindow());
        }, 600);
        return;
      }
      try {
        await invoke("plugin:process|restart");
      } catch (err) {
        console.warn("relaunch failed", err);
      }
    } catch (err) {
      console.error(err);
      setError(err instanceof Error ? err.message : String(err));
      setPhase(Phase.Error);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    updaterApi.pending().then((u) => {
      if (cancelled) return;
      if (u) setInfo(u);
      else updaterApi.check().then((fresh) => !cancelled && setInfo(fresh));
    });
    updaterApi.onProgress(handleProgress).then((un) => {
      unlistenRef.current = un;
    });
    return () => {
      cancelled = true;
      unlistenRef.current?.();
    };
  }, [handleProgress]);

  // Discord-style auto-install: as soon as we know an update is pending,
  // start the download/install without waiting for a click.
  useEffect(() => {
    if (autoInstall && info && !autoStartedRef.current && phase === Phase.Idle) {
      autoStartedRef.current = true;
      void onInstall();
    }
  }, [autoInstall, info, phase, onInstall]);

  const onLater = useCallback(() => {
    const skipPromise = skipVersion && info
      ? updaterApi.setSkippedVersion(info.version)
      : Promise.resolve();
    void skipPromise.finally(() => {
      updaterApi.dismiss().catch(() => updaterApi.closeWindow());
    });
  }, [skipVersion, info]);

  const percent = useMemo(() => {
    if (!total || total <= 0) return null;
    return Math.min(100, Math.round((downloaded / total) * 100));
  }, [downloaded, total]);

  const busy = phase === Phase.Downloading || phase === Phase.Installing;
  const heading = useMemo(() => getHeading(phase), [phase]);
  const subtitle = useMemo(
    () => getSubtitle(phase, info, autoInstall),
    [phase, info, autoInstall],
  );
  const progressText = useMemo(
    () => getProgressText(phase, percent),
    [phase, percent],
  );
  const progressFillStyle = percent == null ? undefined : { width: `${percent}%` };

  return (
    <div className={styles.root}>
      <button
        type="button"
        className={styles.closeBtn}
        onClick={onLater}
        aria-label="Close"
        disabled={busy}
        title={busy ? "Update in progress..." : "Close"}
      >
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <path
            d="M2 2 L12 12 M12 2 L2 12"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
          />
        </svg>
      </button>
      <div className={styles.body}>
        <BrandLogo size={72} className={styles.logo} />
        <h1 className={styles.title}>{heading}</h1>
        <p className={styles.subtitle}>{subtitle}</p>

        {info && (
          <div className={styles.versions}>
            <span className={styles.versionPill}>v{info.current_version}</span>
            <span aria-hidden="true">&rarr;</span>
            <span className={`${styles.versionPill} ${styles.next}`}>v{info.version}</span>
          </div>
        )}

        {info?.body && phase === Phase.Idle && !autoInstall && (
          <pre className={styles.notes}>{info.body}</pre>
        )}

        {error && <div className={styles.error}>{error}</div>}

        {busy && (
          <div className={styles.progressWrap}>
            <div className={styles.progressBar}>
              <div
                className={`${styles.progressFill} ${
                  percent == null ? styles.indeterminate : ""
                }`}
                style={progressFillStyle}
              />
            </div>
            <div className={styles.progressLabel}>
              <span>{progressText}</span>
              {phase === Phase.Downloading && total != null && (
                <span>{formatBytes(downloaded)} / {formatBytes(total)}</span>
              )}
            </div>
          </div>
        )}

      </div>

      {!autoInstall && (
        <div className={styles.footer}>
          {info && phase === Phase.Idle && (
            <button
              type="button"
              className={styles.skipLink}
              onClick={() => setSkipVersion((v) => !v)}
              aria-pressed={skipVersion}
            >
              {skipVersion
                ? `\u2713 Skipping v${info.version} \u2014 click to undo`
                : `Skip v${info.version}`}
            </button>
          )}
          <div className={styles.actions}>
            <button
              type="button"
              className={`${styles.btn} ${styles.btnSecondary}`}
              onClick={onLater}
              disabled={busy}
            >
              {phase === Phase.Error ? "Close" : "Later"}
            </button>
            <button
              type="button"
              className={`${styles.btn} ${styles.btnPrimary}`}
              onClick={onInstall}
              disabled={!info || busy || phase === Phase.Done}
            >
              {phase === Phase.Error ? "Retry" : "Update now"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
