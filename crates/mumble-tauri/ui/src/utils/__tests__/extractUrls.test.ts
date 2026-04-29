import { describe, it, expect } from "vitest";
import { extractUrlsFromMessage } from "../extractUrls";

describe("extractUrlsFromMessage", () => {
  it("returns plain http(s) URLs from body text", () => {
    const urls = extractUrlsFromMessage("hello https://example.com/page world");
    expect(urls).toEqual(["https://example.com/page"]);
  });

  it("decodes &amp; in URLs (regression: YouTube preview hangs)", () => {
    // What Tiptap / sanitizer produces when serialising a link whose
    // href contains an ampersand.
    const body =
      '<p><a href="https://www.youtube.com/watch?v=eQLzLc9cgq8&amp;list=RDeQLzLc9cgq8&amp;start_radio=1">' +
      'https://www.youtube.com/watch?v=eQLzLc9cgq8&amp;list=RDeQLzLc9cgq8&amp;start_radio=1</a></p>';
    const urls = extractUrlsFromMessage(body);
    expect(urls).toContain(
      "https://www.youtube.com/watch?v=eQLzLc9cgq8&list=RDeQLzLc9cgq8&start_radio=1",
    );
    expect(urls.some((u) => u.includes("&amp;"))).toBe(false);
  });

  it("strips HTML tags and finds URLs in link text", () => {
    // Tiptap / sanitiser serialises links with the URL also as text content,
    // so stripping tags still leaves the URL reachable.
    const urls = extractUrlsFromMessage(
      '<a href="https://a.example/x">https://a.example/x</a> and https://b.example/y',
    );
    expect(urls.sort()).toEqual([
      "https://a.example/x",
      "https://b.example/y",
    ]);
  });

  it("decodes numeric and named entities", () => {
    const urls = extractUrlsFromMessage(
      "https://x.test/?a=1&#38;b=2 and https://y.test/?p=q&#x26;r=s",
    );
    expect(urls).toContain("https://x.test/?a=1&b=2");
    expect(urls).toContain("https://y.test/?p=q&r=s");
  });

  it("returns empty array for body with no URLs", () => {
    expect(extractUrlsFromMessage("just some text")).toEqual([]);
    expect(extractUrlsFromMessage("<p>hi <b>there</b></p>")).toEqual([]);
  });
});
