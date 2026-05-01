/**
 * Replace every character in `value` with `*` so identifying strings
 * (host names, IP addresses, etc.) cannot be read off a screen capture.
 *
 * Returns an empty string when `value` is empty.
 */
export function maskSensitive(value: string | number | null | undefined): string {
  if (value === null || value === undefined) return "";
  const str = String(value);
  if (!str) return "";
  return "*".repeat(Math.min(str.length, 12));
}
