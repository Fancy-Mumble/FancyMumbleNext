/**
 * Detect a startable video source inside a chat message body.
 *
 * Used by `MessageItem` to decide whether to surface the
 * "Watch together" action on a message.  Two source kinds are
 * supported:
 *   - `directMedia` — a direct media URL (anything with a recognised
 *     video extension, or an existing `<video>` data-URL embed).
 *   - `youtube` — a YouTube watch / shorts / `youtu.be` link, only
 *     when the user has opted in to external embeds.
 *
 * Returns the highest-priority candidate found, or `null` when no
 * source is present.  YouTube wins over `directMedia` so that posting
 * a YouTube link does not get hijacked by an unrelated `.mp4`
 * elsewhere in the body.
 */

import type { WatchSourceKind } from "../components/chat/watch/watchTypes";

/** A startable video source detected in a message body. */
export interface DetectedVideoSource {
  kind: WatchSourceKind;
  url: string;
  /**
   * For YouTube sources, the canonical 11-character video ID.  Lets
   * the YouTube adapter construct the embed URL without re-parsing.
   */
  youtubeId?: string;
  /** Best-effort title (currently the URL path tail or YouTube ID). */
  title: string;
}

const VIDEO_EXTENSIONS = [
  "mp4", "webm", "mov", "mkv", "m4v", "ogv", "avi",
];

const VIDEO_EXT_RE = new RegExp(
  `\\.(${VIDEO_EXTENSIONS.join("|")})(?:\\?|#|$)`,
  "i",
);

const YOUTUBE_RE =
  /(?:youtube\.com\/(?:watch\?v=|shorts\/|embed\/|v\/)|youtu\.be\/)([a-zA-Z0-9_-]{11})/i;

const URL_RE = /https?:\/\/[^\s<>"')\]]+/gi;

/**
 * Strip HTML tags and decode the small entity set produced by the
 * Mumble HTML serializer so URLs in href attributes become
 * matchable text.
 */
function decode(body: string): string {
  return body
    .replaceAll(/<[^>]+>/g, " ")
    .replaceAll("&amp;", "&")
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&quot;", "\"")
    .replaceAll("&#39;", "'");
}

function tailFromUrl(url: string): string {
  try {
    const u = new URL(url);
    const last = u.pathname.split("/").filter(Boolean).pop();
    return last ?? u.hostname;
  } catch {
    return url;
  }
}

/**
 * Find the first watch-together-eligible video source in `body`.
 *
 * @param body Raw message HTML body.
 * @param allowExternal When false, only `directMedia` (data URLs and
 *        direct media links) are considered; YouTube links are
 *        ignored.  Mirrors the `enableExternalEmbeds` preference.
 */
export function detectVideoSource(
  body: string,
  allowExternal: boolean,
): DetectedVideoSource | null {
  if (allowExternal) {
    const yt = YOUTUBE_RE.exec(body);
    if (yt) {
      return {
        kind: "youtube",
        url: `https://www.youtube.com/watch?v=${yt[1]}`,
        youtubeId: yt[1],
        title: yt[1],
      };
    }
  }

  const text = decode(body);
  for (const match of text.matchAll(URL_RE)) {
    const url = match[0];
    if (VIDEO_EXT_RE.test(url)) {
      return { kind: "directMedia", url, title: tailFromUrl(url) };
    }
  }

  // Embedded <video src="data:video/...;base64,..."> from a local upload.
  const videoTag = /<video[^>]+src="(data:video\/[^"]+)"/i.exec(body);
  if (videoTag) {
    return { kind: "directMedia", url: videoTag[1], title: "video" };
  }

  return null;
}
