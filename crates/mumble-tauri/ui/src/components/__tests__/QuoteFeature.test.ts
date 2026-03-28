/**
 * Unit tests for the message quote/cite feature.
 *
 * These test the pure-logic aspects of quote handling:
 *   - Quote marker extraction (FANCY_QUOTE regex)
 *   - Quote marker stripping from body
 *   - Multiple quotes in a single message
 *   - Body with quotes and remaining text
 *   - Quote-only messages (no remaining body)
 *   - previewText helper (HTML stripping + truncation)
 */

import { describe, it, expect } from "vitest";

// --- Helpers (replicating logic from components) -----------------

/** Regex to match quote reference markers in message bodies. */
const QUOTE_RE = /<!-- FANCY_QUOTE:(.+?) -->/g;

/** Extract all quote message IDs from a message body. */
function extractQuoteIds(body: string): string[] {
  const ids: string[] = [];
  for (const m of body.matchAll(QUOTE_RE)) ids.push(m[1]);
  return ids;
}

/** Strip all quote markers from a message body. */
function stripQuoteMarkers(body: string): string {
  return body.replaceAll(QUOTE_RE, "").trim();
}

/** Replicate the previewText helper from QuoteBlock. */
function previewText(html: string, maxLen = 120): string {
  const text = html
    .replaceAll(/<!--[\s\S]*?-->/g, "")
    .replaceAll(/<[^>]*>/g, "")
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&amp;", "&")
    .trim();
  if (text.length <= maxLen) return text;
  return text.slice(0, maxLen) + "\u2026";
}

/** Replicate the quote marker prepend logic from handleSend. */
function buildMessageBody(
  draftText: string,
  pendingQuoteIds: string[],
  markdownToHtml: (s: string) => string,
): string {
  const quoteMarkers = pendingQuoteIds
    .map((id) => `<!-- FANCY_QUOTE:${id} -->`)
    .join("");
  const htmlBody = draftText.trim() ? markdownToHtml(draftText.trim()) : "";
  return quoteMarkers + htmlBody;
}

/** Replicate the thumbnail extraction from QuoteBlock. */
function extractThumbnailSrc(html: string): string | null {
  const imgMatch = /<img[^>]+src="([^"]+)"/i.exec(html);
  if (imgMatch) return imgMatch[1];
  const vidMatch = /<video[^>]+src="([^"]+)"/i.exec(html);
  if (vidMatch) return vidMatch[1];
  const sourceMatch = /<source[^>]+src="([^"]+)"/i.exec(html);
  return sourceMatch ? sourceMatch[1] : null;
}

// --- Quote marker extraction -------------------------------------

describe("quote marker extraction", () => {
  it("extracts a single quote ID", () => {
    const body = "<!-- FANCY_QUOTE:abc-123 -->Hello world";
    expect(extractQuoteIds(body)).toEqual(["abc-123"]);
  });

  it("extracts multiple quote IDs", () => {
    const body =
      "<!-- FANCY_QUOTE:id-1 --><!-- FANCY_QUOTE:id-2 -->Some text";
    expect(extractQuoteIds(body)).toEqual(["id-1", "id-2"]);
  });

  it("returns empty array when no quotes present", () => {
    const body = "<b>Hello</b> world";
    expect(extractQuoteIds(body)).toEqual([]);
  });

  it("handles UUIDs as quote IDs", () => {
    const uuid = "550e8400-e29b-41d4-a716-446655440000";
    const body = `<!-- FANCY_QUOTE:${uuid} -->`;
    expect(extractQuoteIds(body)).toEqual([uuid]);
  });

  it("does not confuse poll markers with quote markers", () => {
    const body =
      "<!-- FANCY_POLL:poll-1 --><!-- FANCY_QUOTE:quote-1 -->";
    expect(extractQuoteIds(body)).toEqual(["quote-1"]);
  });
});

// --- Quote marker stripping --------------------------------------

describe("quote marker stripping", () => {
  it("strips a single quote marker", () => {
    const body = "<!-- FANCY_QUOTE:abc-123 -->Hello world";
    expect(stripQuoteMarkers(body)).toBe("Hello world");
  });

  it("strips multiple quote markers", () => {
    const body =
      "<!-- FANCY_QUOTE:id-1 --><!-- FANCY_QUOTE:id-2 -->Some <b>text</b>";
    expect(stripQuoteMarkers(body)).toBe("Some <b>text</b>");
  });

  it("returns empty string for quote-only body", () => {
    const body = "<!-- FANCY_QUOTE:id-1 -->";
    expect(stripQuoteMarkers(body)).toBe("");
  });

  it("preserves poll markers when stripping quotes", () => {
    const body =
      "<!-- FANCY_QUOTE:q-1 --><!-- FANCY_POLL:p-1 -->";
    expect(stripQuoteMarkers(body)).toBe("<!-- FANCY_POLL:p-1 -->");
  });
});

// --- Preview text helper -----------------------------------------

describe("previewText", () => {
  it("strips HTML tags from body", () => {
    expect(previewText("<b>Hello</b> <i>world</i>")).toBe("Hello world");
  });

  it("strips HTML comment markers", () => {
    expect(
      previewText("<!-- FANCY_QUOTE:x -->Some text"),
    ).toBe("Some text");
  });

  it("truncates long text with ellipsis", () => {
    const longText = "A".repeat(200);
    const result = previewText(longText, 120);
    expect(result).toHaveLength(121); // 120 chars + ellipsis
    expect(result.endsWith("\u2026")).toBe(true);
  });

  it("does not truncate short text", () => {
    expect(previewText("Short text")).toBe("Short text");
  });

  it("decodes &lt; and &gt; entities", () => {
    expect(previewText("a &lt;b&gt; c")).toBe("a <b> c");
  });

  it("decodes &amp; entity", () => {
    expect(previewText("a &amp; b")).toBe("a & b");
  });

  it("decodes entities after stripping tags", () => {
    expect(previewText("<b>x &lt; y</b>")).toBe("x < y");
  });

  it("returns empty string for purely HTML content", () => {
    expect(previewText('<img src="x.png" />')).toBe("");
  });
});

// --- Message body construction (handleSend logic) ----------------

describe("buildMessageBody", () => {
  const identity = (s: string) => s;

  it("prepends quote markers to HTML body", () => {
    const body = buildMessageBody("Hello", ["q-1"], identity);
    expect(body).toBe("<!-- FANCY_QUOTE:q-1 -->Hello");
  });

  it("prepends multiple quote markers", () => {
    const body = buildMessageBody("Hello", ["q-1", "q-2"], identity);
    expect(body).toBe(
      "<!-- FANCY_QUOTE:q-1 --><!-- FANCY_QUOTE:q-2 -->Hello",
    );
  });

  it("returns only quote markers when draft is empty", () => {
    const body = buildMessageBody("", ["q-1"], identity);
    expect(body).toBe("<!-- FANCY_QUOTE:q-1 -->");
  });

  it("returns only HTML when no quotes", () => {
    const body = buildMessageBody("Hello", [], identity);
    expect(body).toBe("Hello");
  });

  it("round-trips: build then extract", () => {
    const body = buildMessageBody("Reply text", ["q-1", "q-2"], identity);
    const ids = extractQuoteIds(body);
    const remaining = stripQuoteMarkers(body);
    expect(ids).toEqual(["q-1", "q-2"]);
    expect(remaining).toBe("Reply text");
  });
});

// --- Thumbnail extraction ----------------------------------------

describe("extractThumbnailSrc", () => {
  it("extracts src from an img tag", () => {
    const html = '<img src="data:image/png;base64,ABC" alt="photo" />';
    expect(extractThumbnailSrc(html)).toBe("data:image/png;base64,ABC");
  });

  it("extracts src from a video tag", () => {
    const html = '<video src="data:video/mp4;base64,XYZ" controls></video>';
    expect(extractThumbnailSrc(html)).toBe("data:video/mp4;base64,XYZ");
  });

  it("extracts src from a source tag inside video", () => {
    const html = '<video><source src="data:video/webm;base64,123" /></video>';
    expect(extractThumbnailSrc(html)).toBe("data:video/webm;base64,123");
  });

  it("prefers img over video when both present", () => {
    const html = '<img src="img.png" /><video src="vid.mp4"></video>';
    expect(extractThumbnailSrc(html)).toBe("img.png");
  });

  it("returns null for text-only messages", () => {
    expect(extractThumbnailSrc("<b>Hello</b> world")).toBeNull();
  });

  it("returns null for empty body", () => {
    expect(extractThumbnailSrc("")).toBeNull();
  });

  it("extracts src with mixed text and media", () => {
    const html = 'Check this out <img src="photo.jpg" /> nice!';
    expect(extractThumbnailSrc(html)).toBe("photo.jpg");
  });
});
