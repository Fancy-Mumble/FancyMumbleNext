/**
 * Security-focused tests for sanitizeHtml.
 *
 * Each section tries to break the sanitizer with known XSS vectors,
 * dangerous URL schemes, CSS injection, tag smuggling, and other
 * attack patterns.
 */

import { describe, it, expect } from "vitest";
import { sanitizeHtml } from "../sanitizeHtml";

// ─── Helpers ──────────────────────────────────────────────────────

/** Parse the output into a temporary DOM so we can query it. */
function parse(html: string): Document {
  return new DOMParser().parseFromString(
    `<body>${sanitizeHtml(html)}</body>`,
    "text/html",
  );
}

/** Shorthand: sanitize and check the result contains no trace of `needle`. */
function expectStripped(input: string, needle: string) {
  const out = sanitizeHtml(input);
  expect(out.toLowerCase()).not.toContain(needle.toLowerCase());
}

// ─── Basic plumbing ───────────────────────────────────────────────

describe("sanitizeHtml basics", () => {
  it("returns empty string for empty input", () => {
    expect(sanitizeHtml("")).toBe("");
  });

  it("returns empty string for null-ish input", () => {
    expect(sanitizeHtml(null as unknown as string)).toBe("");
    expect(sanitizeHtml(undefined as unknown as string)).toBe("");
  });

  it("preserves plain text", () => {
    expect(sanitizeHtml("hello world")).toBe("hello world");
  });

  it("preserves allowed inline formatting", () => {
    const input = "<b>bold</b> <i>italic</i> <u>underline</u> <s>strike</s>";
    const out = sanitizeHtml(input);
    expect(out).toContain("<b>bold</b>");
    expect(out).toContain("<i>italic</i>");
    expect(out).toContain("<u>underline</u>");
    expect(out).toContain("<s>strike</s>");
  });

  it("preserves block-level tags", () => {
    const out = sanitizeHtml("<h1>Title</h1><p>Paragraph</p><blockquote>Quote</blockquote>");
    expect(out).toContain("<h1>");
    expect(out).toContain("<p>");
    expect(out).toContain("<blockquote>");
  });

  it("preserves table structure", () => {
    const input = "<table><thead><tr><th>A</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table>";
    const out = sanitizeHtml(input);
    expect(out).toContain("<table>");
    expect(out).toContain("<th>A</th>");
    expect(out).toContain("<td>1</td>");
  });

  it("preserves list structure", () => {
    const out = sanitizeHtml("<ul><li>one</li><li>two</li></ul>");
    expect(out).toContain("<ul>");
    expect(out).toContain("<li>one</li>");
  });
});

// ─── Script injection ─────────────────────────────────────────────

describe("script injection", () => {
  it("strips <script> tags completely", () => {
    expectStripped('<script>alert("xss")</script>', "<script");
    expectStripped('<script>alert("xss")</script>', "alert");
  });

  it("strips <script> with src attribute", () => {
    expectStripped('<script src="https://evil.com/xss.js"></script>', "<script");
    expectStripped('<script src="https://evil.com/xss.js"></script>', "evil.com");
  });

  it("strips nested scripts", () => {
    expectStripped('<div><script>document.cookie</script></div>', "document.cookie");
  });

  it("strips <script> with unusual casing", () => {
    expectStripped('<ScRiPt>alert(1)</sCrIpT>', "alert");
  });

  it("strips <script> with extra whitespace", () => {
    expectStripped('<script  \n >alert(1)</script >', "alert");
  });

  it("strips scripts hidden inside other tags", () => {
    expectStripped('<img src=x onerror="alert(1)">', "alert");
  });
});

// ─── Event handler attributes ─────────────────────────────────────

describe("event handler attributes", () => {
  it("strips onclick", () => {
    expectStripped('<div onclick="alert(1)">click me</div>', "onclick");
  });

  it("strips onmouseover", () => {
    expectStripped('<span onmouseover="alert(1)">hover</span>', "onmouseover");
  });

  it("strips onerror on img", () => {
    const out = sanitizeHtml('<img src="x" onerror="alert(1)">');
    expect(out).not.toContain("onerror");
    expect(out).not.toContain("alert");
  });

  it("strips onload", () => {
    expectStripped('<body onload="alert(1)">', "onload");
    expectStripped('<img onload="alert(1)" src="data:image/png;base64,iVBOR">', "onload");
  });

  it("strips onfocus and autofocus tricks", () => {
    expectStripped('<input onfocus="alert(1)" autofocus>', "onfocus");
  });

  it("strips onanimationend", () => {
    expectStripped('<div onanimationend="alert(1)">anim</div>', "onanimationend");
  });

  it("strips all on* attributes even unknown ones", () => {
    expectStripped('<div onfoobar="alert(1)">test</div>', "onfoobar");
  });
});

// ─── Dangerous tags ───────────────────────────────────────────────

describe("dangerous tags", () => {
  it("strips <iframe>", () => {
    expectStripped('<iframe src="https://evil.com"></iframe>', "<iframe");
  });

  it("strips <object>", () => {
    expectStripped('<object data="evil.swf"></object>', "<object");
  });

  it("strips <embed>", () => {
    expectStripped('<embed src="evil.swf">', "<embed");
  });

  it("strips <form>", () => {
    expectStripped('<form action="https://evil.com"><input type="submit"></form>', "<form");
  });

  it("strips <input>", () => {
    expectStripped('<input type="text" value="trap">', "<input");
  });

  it("strips <textarea>", () => {
    expectStripped("<textarea>hidden content</textarea>", "<textarea");
  });

  it("strips <button>", () => {
    expectStripped("<button>click me</button>", "<button");
  });

  it("strips <select>", () => {
    expectStripped('<select><option value="1">one</option></select>', "<select");
  });

  it("strips <style>", () => {
    expectStripped("<style>body{display:none}</style>", "<style");
  });

  it("strips <link>", () => {
    expectStripped('<link rel="stylesheet" href="evil.css">', "<link");
  });

  it("strips <meta>", () => {
    expectStripped('<meta http-equiv="refresh" content="0;url=evil.com">', "<meta");
  });

  it("strips <base>", () => {
    expectStripped('<base href="https://evil.com/">', "<base");
  });

  it("strips <svg> (potential XSS vector)", () => {
    expectStripped(
      '<svg onload="alert(1)"><circle r="40"></circle></svg>',
      "<svg",
    );
  });

  it("strips <math> (potential XSS vector)", () => {
    expectStripped("<math><maction actiontype=\"statusline\">xss</maction></math>", "<math");
  });

  it("strips <video> and <audio> tags", () => {
    expectStripped('<video src="evil.mp4" autoplay></video>', "<video");
    expectStripped('<audio src="evil.mp3" autoplay></audio>', "<audio");
  });
});

// ─── Anchor href attacks ──────────────────────────────────────────

describe("anchor href attacks", () => {
  it("preserves valid https links and marks them external", () => {
    const doc = parse('<a href="https://example.com">link</a>');
    const a = doc.querySelector("a");
    expect(a).not.toBeNull();
    expect(a!.getAttribute("href")).toBe("https://example.com");
    expect(a!.dataset["external"]).toBe("true");
    expect(a!.getAttribute("target")).toBe("_blank");
    expect(a!.getAttribute("rel")).toBe("noopener noreferrer");
  });

  it("preserves valid http links and marks them external", () => {
    const doc = parse('<a href="http://example.com">link</a>');
    const a = doc.querySelector("a");
    expect(a).not.toBeNull();
    expect(a!.getAttribute("href")).toBe("http://example.com");
    expect(a!.dataset["external"]).toBe("true");
  });

  it("strips javascript: href but keeps link text", () => {
    const out = sanitizeHtml('<a href="javascript:alert(1)">click</a>');
    expect(out).not.toContain("javascript:");
    expect(out).toContain("click");
    expect(out).not.toContain("<a");
  });

  it("strips javascript: with mixed case", () => {
    const out = sanitizeHtml('<a href="JaVaScRiPt:alert(1)">xss</a>');
    expect(out).not.toContain("javascript");
    expect(out).toContain("xss");
  });

  it("strips javascript: with leading whitespace", () => {
    const out = sanitizeHtml('<a href="  javascript:alert(1)">xss</a>');
    expect(out).not.toContain("javascript");
  });

  it("strips javascript: with tab/newline obfuscation", () => {
    const out = sanitizeHtml('<a href="java\tscri\npt:alert(1)">xss</a>');
    expect(out).not.toContain("alert");
  });

  it("strips javascript: with HTML entities", () => {
    const out = sanitizeHtml('<a href="&#106;avascript:alert(1)">xss</a>');
    expect(out).not.toContain("javascript");
  });

  it("strips vbscript: href", () => {
    const out = sanitizeHtml('<a href="vbscript:MsgBox(1)">xss</a>');
    expect(out).not.toContain("vbscript");
    expect(out).not.toContain("<a");
  });

  it("strips data: href", () => {
    const out = sanitizeHtml('<a href="data:text/html,<script>alert(1)</script>">xss</a>');
    expect(out).not.toContain("data:text");
    expect(out).not.toContain("<a");
  });

  it("strips file: href", () => {
    const out = sanitizeHtml('<a href="file:///etc/passwd">secret</a>');
    expect(out).not.toContain("file:");
    expect(out).not.toContain("<a");
  });

  it("strips ftp: href", () => {
    const out = sanitizeHtml('<a href="ftp://evil.com/malware">download</a>');
    expect(out).not.toContain("ftp:");
    expect(out).not.toContain("<a");
  });

  it("strips blob: href", () => {
    const out = sanitizeHtml('<a href="blob:https://evil.com/uuid">xss</a>');
    expect(out).not.toContain("blob:");
    expect(out).not.toContain("<a");
  });

  it("strips empty href", () => {
    const out = sanitizeHtml('<a href="">click</a>');
    expect(out).not.toContain("<a");
    expect(out).toContain("click");
  });

  it("strips anchor with no href", () => {
    const out = sanitizeHtml("<a>naked link</a>");
    expect(out).not.toContain("<a");
    expect(out).toContain("naked link");
  });

  it("handles multiple anchors with mixed hrefs", () => {
    const out = sanitizeHtml(
      '<a href="https://good.com">good</a> ' +
      '<a href="javascript:alert(1)">bad</a> ' +
      '<a href="https://also-good.com">also good</a>',
    );
    const doc = new DOMParser().parseFromString(`<body>${out}</body>`, "text/html");
    const anchors = doc.querySelectorAll("a");
    expect(anchors).toHaveLength(2);
    expect(anchors[0].getAttribute("href")).toBe("https://good.com");
    expect(anchors[1].getAttribute("href")).toBe("https://also-good.com");
    expect(out).toContain("bad");
    expect(out).not.toContain("javascript");
  });
});

// ─── Image src attacks ────────────────────────────────────────────

describe("image src attacks", () => {
  it("preserves data:image/png src", () => {
    const out = sanitizeHtml('<img src="data:image/png;base64,iVBORw0KGgo=" alt="ok">');
    expect(out).toContain("<img");
    expect(out).toContain("data:image/png;base64,");
  });

  it("preserves data:image/jpeg src", () => {
    const out = sanitizeHtml('<img src="data:image/jpeg;base64,/9j/4AAQ" alt="ok">');
    expect(out).toContain("<img");
  });

  it("preserves data:image/gif src", () => {
    const out = sanitizeHtml('<img src="data:image/gif;base64,R0lGODlh" alt="ok">');
    expect(out).toContain("<img");
  });

  it("preserves data:image/webp src", () => {
    const out = sanitizeHtml('<img src="data:image/webp;base64,UklGR" alt="ok">');
    expect(out).toContain("<img");
  });

  it("removes external http image (IP leak vector)", () => {
    const out = sanitizeHtml('<img src="https://evil.com/tracker.png" alt="track">');
    expect(out).not.toContain("<img");
    expect(out).not.toContain("evil.com");
  });

  it("removes external http image", () => {
    const out = sanitizeHtml('<img src="http://evil.com/img.jpg">');
    expect(out).not.toContain("<img");
  });

  it("removes img with javascript: src", () => {
    const out = sanitizeHtml('<img src="javascript:alert(1)">');
    expect(out).not.toContain("<img");
    expect(out).not.toContain("javascript");
  });

  it("removes img with data:text/html src (not image)", () => {
    const out = sanitizeHtml('<img src="data:text/html,<script>alert(1)</script>">');
    expect(out).not.toContain("<img");
    expect(out).not.toContain("alert");
  });

  it("removes img with data:image/svg+xml src that contains script", () => {
    // SVG data URLs can contain scripts; we allow the MIME but DOMPurify
    // strips the content anyway. The important thing is no script executes.
    const svg = btoa('<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"></svg>');
    const out = sanitizeHtml(`<img src="data:image/svg+xml;base64,${svg}">`);
    // Either the img is kept (SVG data URL is allowed) or removed,
    // but no script content should appear in the output
    expect(out).not.toContain("alert");
    expect(out).not.toContain("onload");
  });

  it("removes img with empty src", () => {
    const out = sanitizeHtml('<img src="">');
    expect(out).not.toContain("<img");
  });

  it("removes img with no src", () => {
    const out = sanitizeHtml("<img>");
    expect(out).not.toContain("<img");
  });

  it("removes img with file: src", () => {
    const out = sanitizeHtml('<img src="file:///etc/passwd">');
    expect(out).not.toContain("<img");
    expect(out).not.toContain("file:");
  });

  it("removes img with protocol-relative src", () => {
    const out = sanitizeHtml('<img src="//evil.com/track.png">');
    expect(out).not.toContain("<img");
  });
});

// ─── CSS injection ────────────────────────────────────────────────

describe("CSS injection", () => {
  it("preserves safe inline styles", () => {
    const out = sanitizeHtml('<span style="color: red; font-size: 14px">text</span>');
    expect(out).toContain("color");
    expect(out).toContain("font-size");
  });

  it("strips position from style", () => {
    const out = sanitizeHtml('<div style="position: fixed; top: 0; left: 0">overlay</div>');
    expect(out).not.toContain("position");
    expect(out).not.toContain("top");
    expect(out).not.toContain("left");
  });

  it("strips url() from style values", () => {
    const out = sanitizeHtml('<div style="background: url(https://evil.com/track.gif)">x</div>');
    expect(out).not.toContain("url(");
    expect(out).not.toContain("evil.com");
  });

  it("strips expression() from style (IE XSS)", () => {
    const out = sanitizeHtml('<div style="width: expression(alert(1))">x</div>');
    expect(out).not.toContain("expression");
    expect(out).not.toContain("alert");
  });

  it("strips javascript: in style values", () => {
    const out = sanitizeHtml('<div style="background: javascript:alert(1)">x</div>');
    expect(out).not.toContain("javascript");
  });

  it("strips @import in style values", () => {
    const out = sanitizeHtml('<div style="@import url(evil.css)">x</div>');
    expect(out).not.toContain("@import");
  });

  it("strips -moz-binding (Firefox XSS)", () => {
    const out = sanitizeHtml('<div style="-moz-binding: url(evil.xml#xss)">x</div>');
    expect(out).not.toContain("-moz-binding");
  });

  it("strips behavior (IE XSS)", () => {
    const out = sanitizeHtml('<div style="behavior: url(evil.htc)">x</div>');
    expect(out).not.toContain("behavior");
  });

  it("allows safe visual properties together", () => {
    const input = '<p style="color: blue; font-weight: bold; text-align: center; margin: 10px">text</p>';
    const out = sanitizeHtml(input);
    expect(out).toContain("color");
    expect(out).toContain("font-weight");
    expect(out).toContain("text-align");
    expect(out).toContain("margin");
  });

  it("strips only dangerous properties and keeps safe ones", () => {
    const input = '<span style="color: red; position: absolute; font-size: 16px">mixed</span>';
    const out = sanitizeHtml(input);
    expect(out).toContain("color");
    expect(out).toContain("font-size");
    expect(out).not.toContain("position");
  });
});

// ─── Data attribute stripping ─────────────────────────────────────

describe("data attributes", () => {
  it("strips custom data-* attributes from input", () => {
    const out = sanitizeHtml('<div data-payload="evil" data-x="y">text</div>');
    expect(out).not.toContain("data-payload");
    expect(out).not.toContain("data-x");
  });

  it("adds data-external to safe anchors (output only)", () => {
    const out = sanitizeHtml('<a href="https://example.com">link</a>');
    expect(out).toContain('data-external="true"');
  });

  it("does not add data-external to stripped anchors", () => {
    const out = sanitizeHtml('<a href="javascript:void(0)">link</a>');
    expect(out).not.toContain("data-external");
  });
});

// ─── Encoding / obfuscation bypasses ──────────────────────────────

describe("encoding and obfuscation attacks", () => {
  it("handles HTML-entity-encoded script tag", () => {
    expectStripped("&lt;script&gt;alert(1)&lt;/script&gt;", "<script");
  });

  it("handles double-encoded entities", () => {
    const out = sanitizeHtml("&amp;lt;script&amp;gt;alert(1)&amp;lt;/script&amp;gt;");
    expect(out).not.toContain("<script");
  });

  it("handles null byte injection attempts", () => {
    const out = sanitizeHtml('<scr\0ipt>alert(1)</scr\0ipt>');
    // The broken tag is stripped; the inner text may survive as plain
    // text which is safe (not executable). The key assertion is that
    // no <script> element is produced.
    expect(out).not.toContain("<script");
  });

  it("handles UTF-8 homoglyph tricks", () => {
    // Using full-width characters - should not be interpreted as tags
    const out = sanitizeHtml("\uFF1Cscript\uFF1Ealert(1)\uFF1C/script\uFF1E");
    expect(out).not.toContain("<script");
  });

  it("handles backtick in event handler (IE quirk)", () => {
    const out = sanitizeHtml('<div onclick=`alert(1)`>test</div>');
    expect(out).not.toContain("onclick");
    expect(out).not.toContain("alert");
  });
});

// ─── DOM clobbering ───────────────────────────────────────────────

describe("DOM clobbering", () => {
  it("strips name attribute (prevents document.X clobbering)", () => {
    const out = sanitizeHtml('<img name="location" src="data:image/png;base64,iVBOR">');
    expect(out).not.toContain("name=");
  });

  it("strips id attribute", () => {
    const out = sanitizeHtml('<div id="__proto__">test</div>');
    expect(out).not.toContain("id=");
  });

  it("strips form-related attributes", () => {
    const out = sanitizeHtml('<div formaction="https://evil.com">test</div>');
    expect(out).not.toContain("formaction");
  });
});

// ─── Mutation XSS (mXSS) patterns ────────────────────────────────

describe("mutation XSS patterns", () => {
  it("handles noscript bypass", () => {
    const out = sanitizeHtml('<noscript><img src=x onerror="alert(1)"></noscript>');
    expect(out).not.toContain("onerror");
    expect(out).not.toContain("alert");
    expect(out).not.toContain("<noscript");
  });

  it("handles details/summary tags", () => {
    const out = sanitizeHtml('<details open ontoggle="alert(1)"><summary>XSS</summary></details>');
    expect(out).not.toContain("ontoggle");
    expect(out).not.toContain("alert");
  });

  it("handles template tag injection", () => {
    const out = sanitizeHtml('<template><img src=x onerror="alert(1)"></template>');
    expect(out).not.toContain("onerror");
    expect(out).not.toContain("<template");
  });
});

// ─── Nested and chained attacks ───────────────────────────────────

describe("nested and chained attacks", () => {
  it("handles script inside anchor text", () => {
    const out = sanitizeHtml('<a href="https://ok.com"><script>alert(1)</script>Link</a>');
    expect(out).not.toContain("<script");
    expect(out).not.toContain("alert");
    expect(out).toContain("Link");
  });

  it("handles deeply nested dangerous content", () => {
    const out = sanitizeHtml(
      '<div><p><span><b><i><u><div onclick="alert(1)"><script>alert(2)</script>safe text</div></u></i></b></span></p></div>',
    );
    expect(out).not.toContain("onclick");
    expect(out).not.toContain("<script");
    expect(out).not.toContain("alert");
    expect(out).toContain("safe text");
  });

  it("handles img inside anchor with dangerous href", () => {
    const out = sanitizeHtml(
      '<a href="javascript:alert(1)"><img src="data:image/png;base64,iVBOR" alt="pic"></a>',
    );
    expect(out).not.toContain("javascript");
    // The img might be kept (valid data URL) but the anchor must be gone
    expect(out).not.toContain("javascript:");
  });

  it("handles style tag trying to restyle the page", () => {
    const out = sanitizeHtml(
      "<style>* { visibility: hidden } .evil { visibility: visible; position: fixed; top: 0 }</style>" +
      '<div class="evil">Phishing content</div>',
    );
    expect(out).not.toContain("<style");
    expect(out).not.toContain("visibility");
  });

  it("handles SVG with foreignObject XSS", () => {
    const out = sanitizeHtml(
      '<svg><foreignObject><body onload="alert(1)"><b>text</b></body></foreignObject></svg>',
    );
    expect(out).not.toContain("<svg");
    expect(out).not.toContain("onload");
    expect(out).not.toContain("alert");
  });
});

// ─── Font tag (legacy Mumble) ─────────────────────────────────────

describe("font tag handling", () => {
  it("preserves <font> with color attribute", () => {
    const out = sanitizeHtml('<font color="red">colored text</font>');
    expect(out).toContain("<font");
    expect(out).toContain('color="red"');
    expect(out).toContain("colored text");
  });

  it("preserves <font> with face attribute", () => {
    const out = sanitizeHtml('<font face="Arial">text</font>');
    expect(out).toContain('face="Arial"');
  });

  it("preserves <font> with size attribute", () => {
    const out = sanitizeHtml('<font size="3">text</font>');
    expect(out).toContain('size="3"');
  });

  it("strips dangerous attributes on <font>", () => {
    const out = sanitizeHtml('<font color="red" onclick="alert(1)">text</font>');
    expect(out).not.toContain("onclick");
    expect(out).toContain('color="red"');
  });
});

// ─── Edge cases ───────────────────────────────────────────────────

describe("edge cases", () => {
  it("handles extremely long input without crashing", () => {
    const long = "<p>" + "A".repeat(100_000) + "</p>";
    const out = sanitizeHtml(long);
    expect(out).toContain("<p>");
    expect(out.length).toBeGreaterThan(100_000);
  });

  it("handles malformed HTML gracefully", () => {
    const out = sanitizeHtml("<b>unclosed <i>tags <u>everywhere");
    expect(out).toBeTruthy();
    // Should not throw and should produce valid-ish HTML
  });

  it("handles self-closing tags", () => {
    const out = sanitizeHtml("<br/><hr/>");
    expect(out).toContain("<br>");
    expect(out).toContain("<hr>");
  });

  it("handles HTML comments (strips them)", () => {
    const out = sanitizeHtml("before<!-- <script>alert(1)</script> -->after");
    expect(out).not.toContain("<!--");
    expect(out).not.toContain("<script");
    expect(out).toContain("before");
    expect(out).toContain("after");
  });

  it("handles CDATA sections", () => {
    const out = sanitizeHtml("<![CDATA[<script>alert(1)</script>]]>");
    expect(out).not.toContain("<script");
  });

  it("handles processing instructions", () => {
    const out = sanitizeHtml('<?xml version="1.0"?><p>text</p>');
    expect(out).not.toContain("<?xml");
    expect(out).toContain("<p>text</p>");
  });

  it("strips class attribute but allows it (passthrough)", () => {
    const out = sanitizeHtml('<p class="highlight">text</p>');
    expect(out).toContain('class="highlight"');
  });

  it("preserves title attribute", () => {
    const out = sanitizeHtml('<abbr title="HyperText">HTML</abbr>');
    expect(out).toContain('title="HyperText"');
  });

  it("preserves colspan and rowspan on table cells", () => {
    const out = sanitizeHtml('<table><tr><td colspan="2" rowspan="3">cell</td></tr></table>');
    expect(out).toContain('colspan="2"');
    expect(out).toContain('rowspan="3"');
  });
});
