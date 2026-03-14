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

import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ServerInfo, DebugStats } from "../types";
import { getPreferences } from "../preferencesStorage";
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

interface ServerInfoPanelProps {
  readonly onClose: () => void;
}

export default function ServerInfoPanel({ onClose }: ServerInfoPanelProps) {
  const [info, setInfo] = useState<ServerInfo | null>(null);
  const [devMode, setDevMode] = useState(false);
  const [debugStats, setDebugStats] = useState<DebugStats | null>(null);

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

          {/* Developer section (expert + developer mode only) */}
          {devMode && debugStats && (
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
              <div className={styles.infoGrid}>
                <span className={styles.infoLabel}>Channel Messages</span>
                <span className={styles.infoValue}>{debugStats.channel_message_count}</span>

                <span className={styles.infoLabel}>DM Messages</span>
                <span className={styles.infoValue}>{debugStats.dm_message_count}</span>

                <span className={styles.infoLabel}>Group Messages</span>
                <span className={styles.infoValue}>{debugStats.group_message_count}</span>

                <span className={styles.infoLabel}>Total Messages</span>
                <span className={styles.infoValue}>
                  <strong>{debugStats.total_message_count}</strong>
                </span>

                <span className={styles.infoLabel}>Offloaded</span>
                <span className={styles.infoValue}>{debugStats.offloaded_count}</span>

                <span className={styles.infoLabel}>Channels</span>
                <span className={styles.infoValue}>{debugStats.channel_count}</span>

                <span className={styles.infoLabel}>Users</span>
                <span className={styles.infoValue}>{debugStats.user_count}</span>

                <span className={styles.infoLabel}>Groups</span>
                <span className={styles.infoValue}>{debugStats.group_count}</span>

                <span className={styles.infoLabel}>Voice State</span>
                <span className={styles.infoValue}>{debugStats.voice_state}</span>

                <span className={styles.infoLabel}>Connection Epoch</span>
                <span className={styles.infoValue}>{debugStats.connection_epoch}</span>

                <span className={styles.infoLabel}>App Uptime</span>
                <span className={styles.infoValue}>{formatUptime(debugStats.uptime_seconds)}</span>
              </div>
            </section>
          )}
        </>
      )}
    </aside>
  );
}
