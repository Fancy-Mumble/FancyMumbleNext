// Extracts URLs from a chat message body (HTML).
//
// HTML serialises ampersands inside attribute values as `&amp;`.  When we
// strip tags from a message body to feed the raw text into a URL regex, the
// literal `&amp;` (and other entities) must be decoded back to `&` (etc.)
// before being sent to the link-preview backend - otherwise providers like
// YouTube's oEmbed endpoint receive a malformed query string and either
// fail or hang until the request times out.

const URL_RE = /https?:\/\/[^\s<>"')\]]+/gi;

/** Decode the entity set produced by HTML serialisation. */
function decodeHtmlEntities(input: string): string {
  if (!input.includes("&")) return input;
  return input
    .replaceAll(/&#x([0-9a-f]+);/gi, (_, hex: string) => String.fromCodePoint(parseInt(hex, 16)))
    .replaceAll(/&#(\d+);/g, (_, dec: string) => String.fromCodePoint(parseInt(dec, 10)))
    .replaceAll("&quot;", "\"")
    .replaceAll("&apos;", "'")
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&nbsp;", " ")
    .replaceAll("&amp;", "&");
}

/**
 * Extracts all http(s) URLs from a chat message body.
 *
 * - Strips HTML tags so URLs inside `<a href="">` text are reachable.
 * - Decodes HTML entities (e.g. `&amp;` -> `&`) before matching, so the
 *   resulting URLs are usable verbatim by remote APIs.
 */
export function extractUrlsFromMessage(body: string): string[] {
  const stripped = body.replaceAll(/<[^>]+>/g, " ");
  const decoded = decodeHtmlEntities(stripped);
  return [...decoded.matchAll(URL_RE)].map((m) => m[0]);
}
