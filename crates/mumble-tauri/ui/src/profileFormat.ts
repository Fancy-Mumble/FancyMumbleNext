/**
 * FancyMumble profile format - serialisation helpers.
 *
 * The user's Mumble comment stores a JSON payload inside an HTML comment
 * so legacy clients simply hide it.  The visible bio text follows.
 *
 * Format:
 *   <!--FANCY:{"v":1,"decoration":"sparkle",...}-->
 *   (bio HTML here)
 *
 * The `texture` (avatar) uses the standard Mumble `UserState.texture`
 * bytes field and is **not** part of this comment payload.
 *
 * The protobuf `comment` field is `optional string` → must be valid
 * UTF-8.  Binary data (e.g. banner images) is base64-encoded.
 */

import type { FancyProfile } from "./types";

const FANCY_PREFIX = "<!--FANCY:";
const FANCY_SUFFIX = "-->";

/** Build a Mumble comment string from profile data + bio. */
export function serializeProfile(
  profile: FancyProfile,
  bio: string,
): string {
  const payload: FancyProfile = { ...profile, v: 1 };
  // Strip undefined keys for a compact string.
  const json = JSON.stringify(payload, (_k, v) =>
    v === undefined ? undefined : v,
  );
  const marker = `${FANCY_PREFIX}${json}${FANCY_SUFFIX}`;
  return bio ? `${marker}\n${bio}` : marker;
}

/** Parse a Mumble comment → FancyMumble profile + visible bio.
 *
 *  Returns `null` profile when the comment was not written by
 *  FancyMumble (i.e. it's a regular comment from a legacy client).
 */
export function parseComment(comment: string): {
  profile: FancyProfile | null;
  bio: string;
} {
  if (!comment.startsWith(FANCY_PREFIX)) {
    return { profile: null, bio: comment };
  }
  const end = comment.indexOf(FANCY_SUFFIX, FANCY_PREFIX.length);
  if (end === -1) {
    return { profile: null, bio: comment };
  }
  const json = comment.substring(FANCY_PREFIX.length, end);
  const bioStart = end + FANCY_SUFFIX.length;
  const bio = comment.substring(bioStart).replace(/^\n/, "");
  try {
    return { profile: JSON.parse(json) as FancyProfile, bio };
  } catch {
    return { profile: null, bio: comment };
  }
}

/** Convert a `data:` URL to a plain `number[]` suitable for Tauri `Vec<u8>`. */
export function dataUrlToBytes(dataUrl: string): number[] {
  const base64 = dataUrl.split(",")[1] ?? "";
  const binary = atob(base64);
  return Array.from(binary, (c) => c.charCodeAt(0));
}

/**
 * Convert raw texture bytes (as `number[]`) to a data-URL suitable for `<img src>`.
 *
 * Detects JPEG vs PNG from the magic bytes; defaults to `image/png`.
 */
export function textureToDataUrl(bytes: number[]): string {
  if (bytes.length === 0) return "";
  const mime =
    bytes[0] === 0xff && bytes[1] === 0xd8
      ? "image/jpeg"
      : "image/png";
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return `data:${mime};base64,${btoa(binary)}`;
}
