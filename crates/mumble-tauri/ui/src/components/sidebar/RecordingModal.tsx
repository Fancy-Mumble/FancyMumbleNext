/**
 * RecordingModal - Developer-mode modal for recording inbound audio.
 *
 * Features:
 *   - Start / Stop recording controls
 *   - Output format selector (WAV)
 *   - Target directory with folder picker
 *   - Filename template with wildcard support
 *     ({date}, {time}, {datetime}, {host}, {user}, {channel})
 *   - Live elapsed time display while recording
 */

import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import styles from "./RecordingModal.module.css";

type RecordingFormat = "wav";

interface RecordingState {
  is_recording: boolean;
  file_path: string | null;
  elapsed_secs: number;
}

interface RecordingModalProps {
  readonly onClose: () => void;
}

const WILDCARD_HELP =
  "{date} = YYYY-MM-DD, {time} = HH-MM-SS, {datetime} = date + time, " +
  "{host} = server, {user} = username, {channel} = channel name";

function formatElapsed(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  const parts: string[] = [];
  if (h > 0) parts.push(String(h).padStart(2, "0"));
  parts.push(String(m).padStart(2, "0"));
  parts.push(String(s).padStart(2, "0"));
  return parts.join(":");
}

export default function RecordingModal({
  onClose,
}: RecordingModalProps) {
  const [directory, setDirectory] = useState("");
  const [filename, setFilename] = useState("recording_{datetime}_{channel}");
  const [format, setFormat] = useState<RecordingFormat>("wav");
  const [recording, setRecording] = useState(false);
  const [filePath, setFilePath] = useState<string | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Poll recording state on mount to resume UI if already recording.
  useEffect(() => {
    invoke<RecordingState>("get_recording_state").then((state) => {
      if (state.is_recording) {
        setRecording(true);
        setFilePath(state.file_path);
        setElapsed(state.elapsed_secs);
      }
    });
  }, []);

  // Tick elapsed counter while recording.
  useEffect(() => {
    if (recording) {
      timerRef.current = setInterval(() => {
        invoke<RecordingState>("get_recording_state").then((state) => {
          setElapsed(state.elapsed_secs);
          if (!state.is_recording) {
            setRecording(false);
            if (timerRef.current) clearInterval(timerRef.current);
          }
        });
      }, 500);
    }
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [recording]);

  const handleBrowse = useCallback(async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected && typeof selected === "string") {
      setDirectory(selected);
    }
  }, []);

  const handleStart = useCallback(async () => {
    setError(null);
    try {
      const path = await invoke<string>("start_recording", {
        directory,
        filename,
        format,
      });
      setRecording(true);
      setFilePath(path);
      setElapsed(0);
    } catch (e) {
      setError(String(e));
    }
  }, [directory, filename, format]);

  const handleStop = useCallback(async () => {
    setError(null);
    try {
      await invoke<string>("stop_recording");
      setRecording(false);
      if (timerRef.current) clearInterval(timerRef.current);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const canStart = directory.trim().length > 0 && filename.trim().length > 0;

  return (
    <div
      className={styles.backdrop}
      onClick={onClose}
      onKeyDown={(e) => {
        if (e.key === "Escape") onClose();
      }}
      role="presentation"
    >
      <div
        className={styles.modal}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        <div className={styles.header}>
          <h3 className={styles.title}>Record Audio</h3>
          <button className={styles.closeBtn} onClick={onClose}>
            &#x2715;
          </button>
        </div>

        <div className={styles.body}>
          {/* Recording status bar (when recording) */}
          {recording && (
            <div className={styles.statusBar}>
              <span className={styles.recordingDot} />
              <span className={styles.statusText}>
                Recording to {filePath}
              </span>
              <span className={styles.elapsed}>
                {formatElapsed(elapsed)}
              </span>
            </div>
          )}

          {/* Output format */}
          <div className={styles.field}>
            <label className={styles.label}>Format</label>
            <select
              className={styles.select}
              value={format}
              onChange={(e) => setFormat(e.target.value as RecordingFormat)}
              disabled={recording}
            >
              <option value="wav">WAV (lossless, 48 kHz 16-bit PCM)</option>
            </select>
          </div>

          {/* Target directory */}
          <div className={styles.field}>
            <label className={styles.label}>Directory</label>
            <div className={styles.directoryRow}>
              <input
                className={styles.input}
                value={directory}
                onChange={(e) => setDirectory(e.target.value)}
                placeholder="Select output directory..."
                disabled={recording}
              />
              <button
                className={styles.browseBtn}
                onClick={handleBrowse}
                disabled={recording}
              >
                Browse
              </button>
            </div>
          </div>

          {/* Filename template */}
          <div className={styles.field}>
            <label className={styles.label}>Filename</label>
            <input
              className={styles.input}
              value={filename}
              onChange={(e) => setFilename(e.target.value)}
              placeholder="recording_{datetime}"
              disabled={recording}
            />
            <span className={styles.fieldHint}>{WILDCARD_HELP}</span>
          </div>

          {/* Error display */}
          {error && (
            <div className={styles.fieldHint} style={{ color: "#ef4444" }}>
              {error}
            </div>
          )}
        </div>

        <div className={styles.footer}>
          <button className={styles.cancelBtn} onClick={onClose}>
            Close
          </button>
          {recording ? (
            <button className={styles.stopBtn} onClick={handleStop}>
              Stop Recording
            </button>
          ) : (
            <button
              className={styles.recordBtn}
              onClick={handleStart}
              disabled={!canStart}
            >
              Start Recording
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
