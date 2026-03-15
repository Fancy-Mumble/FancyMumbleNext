/**
 * Shared bio HTML sanitization.
 *
 * Security guarantees:
 *  - Only a safe allow-list of tags is permitted.
 *  - <img> is allowed, but its `src` MUST be a data: image URL.
 *    Any external URL would leak the user's IP and could be used
 *    for tracking or SSRF; it is silently removed.
 *  - <a> is allowed, but ONLY with http:// or https:// hrefs.
 *    All other schemes (javascript:, data:, vbscript:, etc.) are
 *    removed.  Allowed anchors are decorated with data-external="true"
 *    so the ExternalLinkGuard component can intercept clicks and show
 *    a confirmation dialog before navigation.
 *  - No event attributes (onerror, onload, ...) survive - DOMPurify strips them.
 *  - No data attributes survive (ALLOW_DATA_ATTR: false).
 *  - All other tags and attributes are stripped.
 */

import DOMPurify from "dompurify";

/** Only these image mime types are accepted as data: URLs. */
const DATA_IMAGE_RE = /^data:image\/(png|jpe?g|gif|webp|avif);base64,/i;

/** Only absolute http(s) URLs are allowed as link targets. */
const EXTERNAL_URL_RE = /^https?:\/\//i;

const BIO_SANITIZE_CONFIG = {
  ALLOWED_TAGS: ["p", "br", "strong", "em", "u", "span", "img", "a"],
  ALLOWED_ATTR: ["style", "src", "alt", "width", "height", "href", "target", "rel"],
  ALLOW_DATA_ATTR: false,
};

/**
 * Sanitize bio HTML.
 *
 * - Strips all dangerous tags / attributes via DOMPurify.
 * - Post-processes img elements: removes any whose src is not a
 *   same-document data: image URL to prevent external requests.
 * - Post-processes a elements: removes any with non-http(s) hrefs,
 *   forces target="_blank" + rel="noopener noreferrer", and marks
 *   surviving anchors with data-external="true" so the UI can
 *   intercept clicks and warn the user before navigating away.
 *
 * Returns a safe HTML string.
 */
export function sanitizeBio(html: string): string {
  if (!html) return "";

  const fragment = DOMPurify.sanitize(html, {
    ...BIO_SANITIZE_CONFIG,
    RETURN_DOM_FRAGMENT: true,
  }) as unknown as DocumentFragment;

  // Validate every img src - any external URL is stripped.
  const imgs = fragment.querySelectorAll("img");
  for (const img of imgs) {
    const src = img.getAttribute("src") ?? "";
    if (!DATA_IMAGE_RE.test(src)) {
      img.remove();
    }
  }

  // Validate every anchor href - non-http(s) hrefs are stripped.
  // Allowed anchors are marked as external so the UI can intercept them.
  const anchors = fragment.querySelectorAll("a");
  for (const anchor of anchors) {
    const href = anchor.getAttribute("href") ?? "";
    if (EXTERNAL_URL_RE.test(href)) {
      // Safe http(s) URL - decorate for ExternalLinkGuard interception.
      anchor.dataset["external"] = "true";
      anchor.setAttribute("target", "_blank");
      anchor.setAttribute("rel", "noopener noreferrer");
    } else {
      // Dangerous scheme (javascript:, data:, etc.) - remove the anchor
      // but keep its text content so the bio is still readable.
      anchor.replaceWith(...anchor.childNodes);
    }
  }

  // Serialize the fragment back to an HTML string.
  const wrapper = document.createElement("div");
  wrapper.appendChild(fragment);
  return wrapper.innerHTML;
}
