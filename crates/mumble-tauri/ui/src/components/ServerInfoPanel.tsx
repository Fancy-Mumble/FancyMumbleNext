/**
 * Right-side panel showing server connection details.
 *
 * Mirrors the layout of UserProfileView (close button, sections,
 * info grid) but displays server metadata instead of a user profile.
 *
 * When Developer Mode is active (Settings > Advanced > Developer Mode),
 * an extra "Developer" section is shown with debug statistics fetched
 * from the backend.
 */

import { useEffect, useState, useCallback, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ServerInfo, DebugStats, AudioSettings } from "../types";
import { getPreferences, getSavedAudioSettings } from "../preferencesStorage";
import styles from "./ServerInfoPanel.module.css";

/** Format a bandwidth value (bits/s) into a human-readable string. */
function formatBandwidth(bitsPerSec: number): string {
  if (bitsPerSec >= 1_000_000) {
    return `${(bitsPerSec / 1_000_000).toFixed(1)} Mbit/s`;
  }
  if (bitsPerSec >= 1_000) {
    return `${(bitsPerSec / 1_000).toFixed(0)} kbit/s`;
  }
  return `${bitsPerSec} bit/s`;
}

/** Format seconds into a human-readable uptime string. */
function formatUptime(seconds: number): string {
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  const parts: string[] = [];
  if (d > 0) parts.push(`${d}d`);
  if (h > 0) parts.push(`${h}h`);
  if (m > 0) parts.push(`${m}m`);
  parts.push(`${s}s`);
  return parts.join(" ");
}

function Accordion({ title, defaultOpen = false, children }: {
  title: string;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className={styles.accordion}>
      <button
        type="button"
        className={styles.accordionHeader}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <svg
          className={`${styles.accordionChevron} ${open ? styles.accordionChevronOpen : ""}`}
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <polyline points="9 18 15 12 9 6" />
        </svg>
        <span>{title}</span>
      </button>
      {open && <div className={styles.accordionBody}>{children}</div>}
    </div>
  );
}

function DebugRow({ label, value }: { label: string; value: string | number | boolean }) {
  return (
    <>
      <span className={styles.debugLabel}>{label}</span>
      <span className={styles.debugValue}>{String(value)}</span>
    </>
  );
}

interface ServerInfoPanelProps {
  readonly onClose: () => void;
}

export default function ServerInfoPanel({ onClose }: ServerInfoPanelProps) {
  const [info, setInfo] = useState<ServerInfo | null>(null);
  const [devMode, setDevMode] = useState(false);
  const [debugStats, setDebugStats] = useState<DebugStats | null>(null);
  const [audioSettings, setAudioSettings] = useState<AudioSettings | null>(null);

  // Load server info and developer-mode preference on mount.
  useEffect(() => {
    invoke<ServerInfo>("get_server_info")
      .then(setInfo)
      .catch((e) => console.error("get_server_info error:", e));

    getPreferences()
      .then((prefs) => {
        if (prefs.userMode === "developer") {
          setDevMode(true);
        }
      })
      .catch(() => {});

    // Load audio settings for the debug overview.
    Promise.all([
      getSavedAudioSettings(),
      invoke<AudioSettings>("get_audio_settings"),
    ]).then(([saved, backend]) => {
      setAudioSettings(saved ?? backend);
    }).catch(() => {});
  }, []);

  // Fetch debug stats when developer mode is active, refresh periodically.
  useEffect(() => {
    if (!devMode) return;

    const fetchStats = () => {
      invoke<DebugStats>("get_debug_stats")
        .then(setDebugStats)
        .catch((e) => console.error("get_debug_stats error:", e));
    };

    fetchStats();
    const interval = setInterval(fetchStats, 2000);
    return () => clearInterval(interval);
  }, [devMode]);

  const handleRefreshStats = useCallback(() => {
    invoke<DebugStats>("get_debug_stats")
      .then(setDebugStats)
      .catch((e) => console.error("get_debug_stats error:", e));
  }, []);

  return (
    <aside className={styles.panel}>
      {/* Close button */}
      <button
        className={styles.closeBtn}
        onClick={onClose}
        aria-label="Close server info"
      >
        <svg
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>

      {/* Header */}
      <div className={styles.header}>
        <div className={styles.serverIcon}>
          <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <rect x="2" y="2" width="20" height="8" rx="2" ry="2" />
            <rect x="2" y="14" width="20" height="8" rx="2" ry="2" />
            <line x1="6" y1="6" x2="6.01" y2="6" />
            <line x1="6" y1="18" x2="6.01" y2="18" />
          </svg>
        </div>
        <h2 className={styles.title}>Server Info</h2>
      </div>

      {info && (
        <>
          {/* Connection section */}
          <section className={styles.section}>
            <h3 className={styles.sectionTitle}>Connection</h3>
            <div className={styles.infoGrid}>
              <span className={styles.infoLabel}>Host</span>
              <span className={styles.infoValue}>{info.host}</span>

              <span className={styles.infoLabel}>Port</span>
              <span className={styles.infoValue}>{info.port}</span>

              <span className={styles.infoLabel}>Users</span>
              <span className={styles.infoValue}>
                {info.user_count}
                {info.max_users != null ? ` / ${info.max_users}` : ""}
              </span>
            </div>
          </section>

          {/* Server section */}
          <section className={styles.section}>
            <h3 className={styles.sectionTitle}>Server</h3>
            <div className={styles.infoGrid}>
              {info.release && (
                <>
                  <span className={styles.infoLabel}>Release</span>
                  <span className={styles.infoValue}>{info.release}</span>
                </>
              )}

              {info.os && (
                <>
                  <span className={styles.infoLabel}>OS</span>
                  <span className={styles.infoValue}>{info.os}</span>
                </>
              )}

              {info.protocol_version && (
                <>
                  <span className={styles.infoLabel}>Protocol</span>
                  <span className={styles.infoValue}>{info.protocol_version}</span>
                </>
              )}

              <span className={styles.infoLabel}>Fancy Mumble</span>
              <span className={styles.infoValue}>
                {info.fancy_version != null
                  ? `v${info.fancy_version}`
                  : "Not supported"}
              </span>
            </div>
          </section>

          {/* Audio section */}
          <section className={styles.section}>
            <h3 className={styles.sectionTitle}>Audio</h3>
            <div className={styles.infoGrid}>
              {info.max_bandwidth != null && (
                <>
                  <span className={styles.infoLabel}>Max Bandwidth</span>
                  <span className={styles.infoValue}>
                    {formatBandwidth(info.max_bandwidth)}
                  </span>
                </>
              )}

              <span className={styles.infoLabel}>Codec</span>
              <span className={styles.infoValue}>
                {info.opus ? "Opus" : "CELT"}
              </span>
            </div>
          </section>

          {/* Developer section (developer mode only) */}
          {devMode && (
            <section className={styles.section}>
              <div className={styles.devHeader}>
                <h3 className={styles.sectionTitle}>Developer</h3>
                <button
                  type="button"
                  className={styles.refreshBtn}
                  onClick={handleRefreshStats}
                  aria-label="Refresh debug stats"
                  title="Refresh"
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="23 4 23 10 17 10" />
                    <polyline points="1 20 1 14 7 14" />
                    <path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15" />
                  </svg>
                </button>
              </div>

              {audioSettings && (
                <Accordion title="Audio Settings">
                  <div className={styles.debugGrid}>
                    <DebugRow label="Input Device" value={audioSettings.selected_device ?? "System default"} />
                    <DebugRow label="Bitrate" value={`${audioSettings.bitrate_bps / 1000} kb/s`} />
                    <DebugRow label="Frame Size" value={`${audioSettings.frame_size_ms} ms`} />
                    <DebugRow label="VAD Threshold" value={`${(audioSettings.vad_threshold * 100).toFixed(1)}%`} />
                    <DebugRow label="Auto Gain" value={audioSettings.auto_gain} />
                    <DebugRow label="Max Gain" value={`${audioSettings.max_gain_db} dB`} />
                    <DebugRow label="Noise Suppression" value={audioSettings.noise_suppression} />
                    <DebugRow label="Gate Close Ratio" value={`${(audioSettings.noise_gate_close_ratio * 100).toFixed(0)}%`} />
                    <DebugRow label="Hold Frames" value={audioSettings.hold_frames} />
                    <DebugRow label="Push to Talk" value={audioSettings.push_to_talk} />
                    {audioSettings.push_to_talk_key && (
                      <DebugRow label="PTT Key" value={audioSettings.push_to_talk_key} />
                    )}
                  </div>
                </Accordion>
              )}

              {debugStats && (
                <>
                  <Accordion title="Connection & State">
                    <div className={styles.debugGrid}>
                      <DebugRow label="Voice State" value={debugStats.voice_state} />
                      <DebugRow label="Connection Epoch" value={debugStats.connection_epoch} />
                      <DebugRow label="App Uptime" value={formatUptime(debugStats.uptime_seconds)} />
                      <DebugRow label="Users" value={debugStats.user_count} />
                      <DebugRow label="Channels" value={debugStats.channel_count} />
                      <DebugRow label="Groups" value={debugStats.group_count} />
                    </div>
                  </Accordion>

                  <Accordion title="Messages">
                    <div className={styles.debugGrid}>
                      <DebugRow label="Channel Messages" value={debugStats.channel_message_count} />
                      <DebugRow label="DM Messages" value={debugStats.dm_message_count} />
                      <DebugRow label="Group Messages" value={debugStats.group_message_count} />
                      <DebugRow label="Total Messages" value={debugStats.total_message_count} />
                      <DebugRow label="Offloaded" value={debugStats.offloaded_count} />
                    </div>
                  </Accordion>
                </>
              )}
            </section>
          )}
        </>
      )}
    </aside>
  );
}
