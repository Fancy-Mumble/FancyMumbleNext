/**
 * Unit tests for message editing feature.
 *
 * These test:
 *   - htmlToMarkdown reverse conversion (HTML -> editable markdown)
 *   - markdownToHtml -> htmlToMarkdown round-trip fidelity
 */

import { describe, it, expect } from "vitest";
import { markdownToHtml, htmlToMarkdown } from "../chat/MarkdownInput";

describe("htmlToMarkdown", () => {
  it("converts <b> tags to **bold**", () => {
    expect(htmlToMarkdown("<b>hello</b>")).toBe("**hello**");
  });

  it("converts <strong> tags to **bold**", () => {
    expect(htmlToMarkdown("<strong>hello</strong>")).toBe("**hello**");
  });

  it("converts <i> tags to *italic*", () => {
    expect(htmlToMarkdown("<i>hello</i>")).toBe("*hello*");
  });

  it("converts <em> tags to *italic*", () => {
    expect(htmlToMarkdown("<em>hello</em>")).toBe("*hello*");
  });

  it("converts <u> tags to __underline__", () => {
    expect(htmlToMarkdown("<u>hello</u>")).toBe("__hello__");
  });

  it("converts <s> tags to ~~strikethrough~~", () => {
    expect(htmlToMarkdown("<s>hello</s>")).toBe("~~hello~~");
  });

  it("converts <code> tags to `code`", () => {
    expect(htmlToMarkdown("<code>foo</code>")).toBe("`foo`");
  });

  it("converts <br> to newlines", () => {
    expect(htmlToMarkdown("line1<br>line2")).toBe("line1\nline2");
  });

  it("strips anchor tags keeping text", () => {
    expect(htmlToMarkdown('<a href="https://example.com">https://example.com</a>'))
      .toBe("https://example.com");
  });

  it("unescapes HTML entities", () => {
    expect(htmlToMarkdown("&lt;div&gt; &amp; stuff")).toBe("<div> & stuff");
  });

  it("strips HTML comments", () => {
    expect(htmlToMarkdown("<!-- FANCY_QUOTE:abc123 -->hello")).toBe("hello");
  });

  it("strips unknown HTML tags", () => {
    expect(htmlToMarkdown("<div>hello</div>")).toBe("hello");
  });
});

describe("markdownToHtml -> htmlToMarkdown round-trip", () => {
  const cases = [
    "hello world",
    "**bold text**",
    "*italic text*",
    "__underlined__",
    "~~strikethrough~~",
    "`inline code`",
    "line one\nline two",
    "mixed **bold** and *italic*",
    "||hidden secret||",
    "before ||spoiler|| after",
  ];

  for (const input of cases) {
    it(`round-trips: ${JSON.stringify(input)}`, () => {
      const html = markdownToHtml(input);
      const back = htmlToMarkdown(html);
      expect(back).toBe(input);
    });
  }

  it("keeps semicolon-containing URL segments when linkifying", () => {
    const input = "https://www.youtube.com/watch?v=LAqgQcnkA1k&;list=RDLAqgQcnkA1k&start_radio=1";
    const html = markdownToHtml(input);

    expect(html).toContain(
      '<a href="https://www.youtube.com/watch?v=LAqgQcnkA1k&amp;;list=RDLAqgQcnkA1k&amp;start_radio=1"',
    );

    const back = htmlToMarkdown(html);
    expect(back).toBe(input);
  });

  it("converts spoiler markdown to a span with the spoiler class", () => {
    const html = markdownToHtml("watch out: ||boo||");
    expect(html).toContain('<span class="spoiler">boo</span>');
  });

  it("converts fenced code blocks with a language hint to <pre><code class=\"language-...\">", () => {
    const html = markdownToHtml("```rust\nfn main() {}\n```");
    expect(html).toContain('<pre><code class="language-rust">fn main() {}</code></pre>');
  });

  it("preserves newlines inside fenced code blocks (not converted to <br>)", () => {
    const html = markdownToHtml("```\nline1\nline2\n```");
    expect(html).toContain("<pre><code>line1\nline2</code></pre>");
    expect(html).not.toContain("line1<br>line2");
  });

  it("round-trips a fenced code block back to fenced markdown", () => {
    const input = "```js\nconst a = 1;\nconst b = 2;\n```";
    const html = markdownToHtml(input);
    const back = htmlToMarkdown(html);
    expect(back).toBe(input);
  });
});
