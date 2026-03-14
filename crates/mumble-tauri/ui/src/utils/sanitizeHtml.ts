/**
 * Shared HTML sanitization for all user-generated content.
 *
 * This is the single source of truth for rendering untrusted HTML
 * (channel descriptions, server welcome text, user bios, chat
 * messages, etc.) safely inside the application.
 *
 * Security guarantees:
 *  - Only a safe allow-list of tags is permitted (DOMPurify).
 *  - All event handler attributes (on*) are stripped by DOMPurify.
 *  - No data attributes survive (ALLOW_DATA_ATTR: false).
 *  - <img> is allowed but `src` MUST be a data: image URL.
 *    External image URLs are removed to prevent IP leaks / tracking.
 *  - <a> is allowed but ONLY with http:// or https:// hrefs.
 *    All other schemes (javascript:, data:, vbscript:, etc.) are
 *    removed.  Allowed anchors are decorated with
 *    `data-external="true"` so the ExternalLinkGuard component can
 *    intercept clicks and show a confirmation dialog.
 *  - Inline CSS `style` attributes are restricted to safe visual
 *    properties only (no `position`, `url()`, `expression()`, etc.).
 */

import DOMPurify from "dompurify";

// -- Regular expressions ------------------------------------------------

/** Accepted image data: URL prefixes. */
const DATA_IMAGE_RE = /^data:image\/(png|jpe?g|gif|webp|avif|svg\+xml);base64,/i;

/** Only absolute http(s) URLs are allowed as link targets. */
const EXTERNAL_URL_RE = /^https?:\/\//i;

// -- DOMPurify allow-lists ----------------------------------------------

/**
 * Generous tag allow-list that covers bios, channel descriptions,
 * server welcome text, and chat messages.
 */
const ALLOWED_TAGS = [
  // Inline formatting
  "b", "i", "u", "s", "em", "strong", "small", "sub", "sup",
  "del", "ins", "mark", "abbr", "code", "span", "font",
  // Block structure
  "p", "br", "hr", "pre", "blockquote",
  "h1", "h2", "h3", "h4", "h5", "h6",
  // Lists
  "ul", "ol", "li",
  // Tables
  "table", "thead", "tbody", "tfoot", "tr", "td", "th",
  // Media (post-processed below)
  "img", "a",
];

const ALLOWED_ATTR = [
  "style", "class", "title",
  // img
  "src", "alt", "width", "height",
  // a
  "href", "target", "rel",
  // font (legacy Mumble HTML)
  "color", "size", "face",
  // table cells
  "colspan", "rowspan",
];

const PURIFY_CONFIG = {
  ALLOWED_TAGS,
  ALLOWED_ATTR,
  ALLOW_DATA_ATTR: false,
};

// -- CSS sanitization ---------------------------------------------------

/** CSS properties allowed in inline `style` attributes. */
const SAFE_CSS_PROPS = new Set([
  "color", "background-color", "background",
  "font-size", "font-weight", "font-style", "font-family",
  "text-decoration", "text-decoration-line", "text-decoration-color",
  "text-align", "text-transform", "text-indent",
  "line-height", "letter-spacing", "word-spacing", "word-break",
  "white-space", "vertical-align",
  "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
  "padding", "padding-top", "padding-right", "padding-bottom", "padding-left",
  "border", "border-color", "border-width", "border-style",
  "border-top", "border-right", "border-bottom", "border-left",
  "border-radius", "border-collapse", "border-spacing",
  "display", "list-style", "list-style-type",
  "width", "max-width", "min-width",
  "height", "max-height", "min-height",
]);

/** CSS value patterns that are never allowed (can execute code or fetch). */
const DANGEROUS_CSS_VALUE_RE = /url\s*\(|expression\s*\(|javascript:|@import/i;

function sanitiseStyle(value: string): string {
  return value
    .split(";")
    .filter((decl) => {
      const colonIdx = decl.indexOf(":");
      if (colonIdx < 0) return false;
      const prop = decl.slice(0, colonIdx).trim().toLowerCase();
      const val = decl.slice(colonIdx + 1);
      return SAFE_CSS_PROPS.has(prop) && !DANGEROUS_CSS_VALUE_RE.test(val);
    })
    .join(";");
}

// -- Main sanitization function -----------------------------------------

/**
 * Sanitize untrusted HTML for safe rendering via `dangerouslySetInnerHTML`.
 *
 * 1. DOMPurify strips disallowed tags, attributes, and event handlers.
 * 2. `<img>` elements with external `src` are removed (only data: URLs
 *    with safe image MIME types are kept).
 * 3. `<a>` elements with non-http(s) `href` are unwrapped (text kept).
 *    Surviving anchors receive `data-external="true"`,
 *    `target="_blank"`, and `rel="noopener noreferrer"`.
 * 4. Inline `style` attributes are filtered to safe CSS properties.
 *
 * The returned string is safe to render inside an `ExternalLinkGuard`.
 */
export function sanitizeHtml(html: string): string {
  if (!html) return "";

  const fragment = DOMPurify.sanitize(html, {
    ...PURIFY_CONFIG,
    RETURN_DOM_FRAGMENT: true,
  }) as unknown as DocumentFragment;

  postProcess(fragment);

  const wrapper = document.createElement("div");
  wrapper.appendChild(fragment);
  return wrapper.innerHTML;
}

// -- Post-processing helpers --------------------------------------------

function postProcess(root: DocumentFragment | Element): void {
  // Validate img src - remove external URLs.
  for (const img of Array.from(root.querySelectorAll("img"))) {
    const src = img.getAttribute("src") ?? "";
    if (!DATA_IMAGE_RE.test(src)) {
      img.remove();
    }
  }

  // Validate anchor href - remove dangerous schemes, mark safe ones.
  for (const anchor of Array.from(root.querySelectorAll("a"))) {
    const href = anchor.getAttribute("href") ?? "";
    if (EXTERNAL_URL_RE.test(href)) {
      anchor.dataset["external"] = "true";
      anchor.setAttribute("target", "_blank");
      anchor.setAttribute("rel", "noopener noreferrer");
    } else {
      anchor.replaceWith(...Array.from(anchor.childNodes));
    }
  }

  // Sanitise inline styles.
  for (const el of Array.from(root.querySelectorAll("[style]"))) {
    const raw = el.getAttribute("style") ?? "";
    const safe = sanitiseStyle(raw);
    if (safe) {
      el.setAttribute("style", safe);
    } else {
      el.removeAttribute("style");
    }
  }
}
