import type { TimeFormat } from "../types";

// -- Duration / uptime ---------------------------------------------

/** Format a number of seconds into a compact human-readable string showing
 *  only the two most significant units (e.g. "2d 5h", "1h 23m", "3m 12s"). */
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
  return parts.slice(0, 2).join(" ");
}

// -- Bandwidth -----------------------------------------------------

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

// -- Timestamp -----------------------------------------------------

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

// -- Avatar colour -------------------------------------------------

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

// -- Date chip -----------------------------------------------------

/**
 * Return a calendar-date key ("YYYY-MM-DD") for an epoch-millis timestamp.
 * Uses the local timezone when `localTime` is true, otherwise UTC.
 */
export function dateKey(epochMs: number, localTime = true): string {
  const d = new Date(epochMs);
  if (!localTime) {
    return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}-${String(d.getUTCDate()).padStart(2, "0")}`;
  }
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/**
 * Format a date for the date-change chip in the chat view.
 * Returns "Today", "Yesterday", or a long date like "March 15, 2026".
 */
/**
 * Format a date string (e.g. "2024-03-15T10:30:00") as a compact relative
 * label such as "3d ago", "2mo ago", etc.  Falls back to the raw string
 * if it cannot be parsed.
 */
export function formatRelativeDate(dateStr: string): string {
  const date = new Date(dateStr);
  if (isNaN(date.getTime())) return dateStr;

  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  if (diffMs < 0) return "just now";

  const minutes = Math.floor(diffMs / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;

  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;

  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;

  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo ago`;

  const years = Math.floor(months / 12);
  return `${years}y ago`;
}

export function formatDateChip(epochMs: number, localTime = true): string {
  const d = new Date(epochMs);
  const now = new Date();

  const toDay = (date: Date, local: boolean) =>
    local
      ? new Date(date.getFullYear(), date.getMonth(), date.getDate())
      : new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()));

  const target = toDay(d, localTime);
  const today = toDay(now, localTime);
  const diffMs = today.getTime() - target.getTime();
  const diffDays = Math.round(diffMs / 86_400_000);

  if (diffDays === 0) return "Today";
  if (diffDays === 1) return "Yesterday";

  const opts: Intl.DateTimeFormatOptions = {
    year: "numeric",
    month: "long",
    day: "numeric",
    ...(localTime ? {} : { timeZone: "UTC" }),
  };
  return d.toLocaleDateString(undefined, opts);
}
