import type { TimeFormat } from "../types";

// ── Duration / uptime ─────────────────────────────────────────────

/** Format a number of seconds into a compact human-readable string (e.g. "2d 5h 3m 12s"). */
export function formatDuration(totalSeconds: number): string {
  const d = Math.floor(totalSeconds / 86400);
  const h = Math.floor((totalSeconds % 86400) / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  const parts: string[] = [];
  if (d > 0) parts.push(`${d}d`);
  if (h > 0) parts.push(`${h}h`);
  if (m > 0) parts.push(`${m}m`);
  if (parts.length === 0 || s > 0) parts.push(`${s}s`);
  return parts.join(" ");
}

// ── Bandwidth ─────────────────────────────────────────────────────

/** Format a bandwidth value (bits/s) into a human-readable string. */
export function formatBandwidth(bitsPerSec: number): string {
  if (bitsPerSec >= 1_000_000) {
    return `${(bitsPerSec / 1_000_000).toFixed(1)} Mbit/s`;
  }
  if (bitsPerSec >= 1_000) {
    return `${(bitsPerSec / 1_000).toFixed(0)} kbit/s`;
  }
  return `${bitsPerSec} bit/s`;
}

// ── Timestamp ─────────────────────────────────────────────────────

/**
 * Format a Unix-epoch-millis timestamp into a short time string.
 *
 * @param epochMs       - Timestamp value (always epoch milliseconds).
 * @param timeFormat    - "12h", "24h", or "auto" (follow OS setting).
 * @param localTime     - When true, display in local timezone (default).
 *                        When false, display in UTC.
 * @param systemUses24h - OS-reported clock format for "auto" mode. When
 *   provided, bypasses the unreliable WebView2 Intl probe on Windows.
 */
export function formatTimestamp(
  epochMs: number,
  timeFormat: TimeFormat = "auto",
  localTime = true,
  systemUses24h?: boolean,
): string {
  const d = new Date(epochMs);
  const opts: Intl.DateTimeFormatOptions = {
    hour: "2-digit",
    minute: "2-digit",
  };

  if (timeFormat === "12h") {
    opts.hour12 = true;
  } else if (timeFormat === "24h") {
    opts.hour12 = false;
  } else if (systemUses24h !== undefined) {
    opts.hour12 = !systemUses24h;
  } else {
    const resolved = new Intl.DateTimeFormat([], { hour: "numeric" }).resolvedOptions();
    opts.hour12 = resolved.hour12 ?? (resolved.hourCycle !== "h23" && resolved.hourCycle !== "h24");
  }

  if (!localTime) opts.timeZone = "UTC";

  return d.toLocaleTimeString(undefined, opts);
}

// ── Avatar colour ─────────────────────────────────────────────────

const AVATAR_COLORS = [
  "#2AABEE",
  "#7c3aed",
  "#22c55e",
  "#f59e0b",
  "#ef4444",
  "#ec4899",
];

/** Deterministic colour for a username (stable hash into a palette). */
export function colorFor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = (name.codePointAt(i) ?? 0) + ((hash << 5) - hash);
  }
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}
