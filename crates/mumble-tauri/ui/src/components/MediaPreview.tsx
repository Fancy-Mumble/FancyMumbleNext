/**
 * MediaPreview - renders inline media (images, GIFs, videos) extracted
 * from a Mumble message body.
 *
 * - Images / GIFs show a small thumbnail; click opens a lightbox.
 * - GIFs auto-play for 4 s on first view, then freeze permanently.
 * - Videos show a poster frame; click opens a lightbox with playback.
 */

import {
  useState,
  useRef,
  useEffect,
  useCallback,
  type ReactNode,
} from "react";
import styles from "./MediaPreview.module.css";

// ─── Types ────────────────────────────────────────────────────────

interface MediaItem {
  kind: "image" | "gif" | "video";
  src: string; // data-URL or remote URL
  alt: string;
}

interface Props {
  /** The raw HTML body of the message. */
  html: string;
  /** Unique key for this message (e.g. index) used to track GIF play state. */
  messageId: string;
}

// ─── Global GIF-played tracker ────────────────────────────────────
// Persists across re-renders and channel switches.  Once a GIF has
// played its 4 s it is marked here so it will never auto-play again.
const playedGifs = new Set<string>();

/** Cached frozen-frame data URLs so re-mounts don't need to reload. */
const frozenFrames = new Map<string, string>();

// ─── Helpers ──────────────────────────────────────────────────────

// ─── HTML Sanitiser (whitelist-based) ──────────────────────────────

/** Tags allowed in message text after media extraction. */
const ALLOWED_TAGS = new Set([
  "b", "i", "u", "s", "em", "strong", "br", "p", "span",
  "font", "code", "pre", "a", "ul", "ol", "li", "blockquote",
  "h1", "h2", "h3", "h4", "h5", "h6", "sub", "sup", "small",
  "del", "ins", "abbr", "mark", "hr", "table", "thead", "tbody",
  "tr", "td", "th",
]);

/** Attributes allowed per-tag; `"*"` key applies to every tag. */
const ALLOWED_ATTRS: Record<string, Set<string>> = {
  "*": new Set(["class", "title"]),
  a: new Set(["href", "target", "rel"]),
  font: new Set(["color", "size", "face"]),
  span: new Set(["style"]),
  td: new Set(["colspan", "rowspan"]),
  th: new Set(["colspan", "rowspan"]),
};

/** Protocols accepted inside `href` attributes. */
const SAFE_URL_RE = /^(?:https?:|mailto:|#)/i;

/** CSS properties allowed in inline `style` attributes. */
const SAFE_CSS_PROPS = new Set([
  "color", "background-color", "background", "font-size",
  "font-weight", "font-style", "font-family", "text-decoration",
  "text-align", "margin", "padding", "border", "display",
  "white-space", "word-break", "line-height", "letter-spacing",
]);

/**
 * Walk the DOM tree produced by DOMParser and strip anything
 * not on the whitelist.  Mutates `root` in place.
 */
function sanitiseTree(root: Element): void {
  // Iterate in reverse so removals don't shift indices.
  const children = Array.from(root.children);
  for (const child of children) {
    const tag = child.tagName.toLowerCase();

    if (!ALLOWED_TAGS.has(tag)) {
      // Unsafe tag → replace with its text content (preserves text, kills markup).
      const text = document.createTextNode(child.textContent ?? "");
      child.replaceWith(text);
      continue;
    }

    // Strip disallowed attributes.
    const globalAllowed = ALLOWED_ATTRS["*"] ?? new Set();
    const tagAllowed = ALLOWED_ATTRS[tag] ?? new Set();
    for (const attr of Array.from(child.attributes)) {
      const name = attr.name.toLowerCase();
      // Always kill event handlers.
      if (name.startsWith("on")) {
        child.removeAttribute(attr.name);
        continue;
      }
      if (!globalAllowed.has(name) && !tagAllowed.has(name)) {
        child.removeAttribute(attr.name);
        continue;
      }
      // Validate href URLs.
      if (name === "href" && !SAFE_URL_RE.test(attr.value.trim())) {
        child.removeAttribute(attr.name);
        continue;
      }
      // Sanitise inline styles.
      if (name === "style") {
        const safe = attr.value
          .split(";")
          .filter((decl) => {
            const prop = decl.split(":")[0]?.trim().toLowerCase() ?? "";
            return SAFE_CSS_PROPS.has(prop);
          })
          .join(";");
        if (safe) {
          child.setAttribute("style", safe);
        } else {
          child.removeAttribute("style");
        }
      }
    }

    // Force safe link behaviour.
    if (tag === "a") {
      child.setAttribute("target", "_blank");
      child.setAttribute("rel", "noopener noreferrer");
    }

    // Recurse into children.
    sanitiseTree(child);
  }
}

/** Parse `<img>` and `<video>` tags out of HTML and classify them. */
function extractMedia(html: string): { cleaned: string; media: MediaItem[] } {
  const media: MediaItem[] = [];
  const parser = new DOMParser();
  const doc = parser.parseFromString(html, "text/html");

  // Images
  doc.querySelectorAll("img").forEach((img) => {
    const src = img.getAttribute("src") ?? "";
    if (!src) return;
    const isGif =
      src.startsWith("data:image/gif") || src.toLowerCase().endsWith(".gif");
    media.push({ kind: isGif ? "gif" : "image", src, alt: img.alt || "" });
    img.remove();
  });

  // Videos
  doc.querySelectorAll("video").forEach((vid) => {
    const src =
      vid.getAttribute("src") ??
      vid.querySelector("source")?.getAttribute("src") ??
      "";
    if (!src) return;
    media.push({ kind: "video", src, alt: "" });
    vid.remove();
  });

  // Sanitise the remaining DOM tree.
  sanitiseTree(doc.body);

  const cleaned = doc.body.innerHTML.trim();
  return { cleaned, media };
}

// ─── Sub-components ───────────────────────────────────────────────

function GifThumb({
  item,
  id,
  onOpen,
}: {
  item: MediaItem;
  id: string;
  onOpen: () => void;
}) {
  const imgRef = useRef<HTMLImageElement>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [frozen, setFrozen] = useState(() => playedGifs.has(id));
  const [posterSrc, setPosterSrc] = useState<string | null>(
    () => frozenFrames.get(id) ?? null,
  );

  /** Snapshot whatever frame the <img> is currently showing → data URL,
   *  cache it, then freeze the display. */
  const captureAndFreeze = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = undefined;
    }
    const img = imgRef.current;
    if (img && img.naturalWidth > 0) {
      const c = document.createElement("canvas");
      c.width = img.naturalWidth;
      c.height = img.naturalHeight;
      c.getContext("2d")?.drawImage(img, 0, 0);
      const url = c.toDataURL();
      frozenFrames.set(id, url);
      setPosterSrc(url);
    }
    setFrozen(true);
  }, [id]);

  /** Start the play countdown (4 s). */
  const startTimer = useCallback(() => {
    timerRef.current = setTimeout(captureAndFreeze, 4000);
  }, [captureAndFreeze]);

  /** Called when the playing <img> finishes loading. */
  const handleImgLoad = useCallback(() => {
    // Only auto-start on first play; replays set their own timer.
    if (!playedGifs.has(id)) {
      playedGifs.add(id);
      startTimer();
    }
  }, [id, startTimer]);

  // For already-played GIFs without a cached frame, load frame 0.
  useEffect(() => {
    if (!frozen || posterSrc) return;
    const img = new Image();
    img.onload = () => {
      const c = document.createElement("canvas");
      c.width = img.naturalWidth;
      c.height = img.naturalHeight;
      c.getContext("2d")?.drawImage(img, 0, 0);
      const url = c.toDataURL();
      frozenFrames.set(id, url);
      setPosterSrc(url);
    };
    img.src = item.src;
  }, [frozen, posterSrc, id, item.src]);

  // Pause on window blur or tab hidden.
  useEffect(() => {
    if (frozen) return;
    const pause = () => captureAndFreeze();
    const onVisChange = () => {
      if (document.hidden) pause();
    };
    window.addEventListener("blur", pause);
    document.addEventListener("visibilitychange", onVisChange);
    return () => {
      window.removeEventListener("blur", pause);
      document.removeEventListener("visibilitychange", onVisChange);
    };
  }, [frozen, captureAndFreeze]);

  // Cleanup timer on unmount.
  useEffect(
    () => () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    },
    [],
  );

  const handleReplay = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (timerRef.current) clearTimeout(timerRef.current);
    setFrozen(false);
    startTimer();
  };

  if (frozen) {
    return (
      <div className={styles.thumbWrap} onClick={onOpen}>
        {posterSrc ? (
          <img className={styles.thumb} src={posterSrc} alt={item.alt} />
        ) : (
          <div className={styles.thumbPlaceholder} />
        )}
        <button className={styles.replayOverlay} onClick={handleReplay}>
          <span className={styles.replayIcon}>▶</span>
        </button>
        <span className={styles.gifBadge}>GIF</span>
      </div>
    );
  }

  return (
    <div className={styles.thumbWrap} onClick={onOpen}>
      <img
        ref={imgRef}
        className={styles.thumb}
        src={item.src}
        alt={item.alt}
        onLoad={handleImgLoad}
      />
      <span className={styles.gifBadge}>GIF</span>
    </div>
  );
}

function ImageThumb({
  item,
  onOpen,
}: {
  item: MediaItem;
  onOpen: () => void;
}) {
  return (
    <div className={styles.thumbWrap} onClick={onOpen}>
      <img className={styles.thumb} src={item.src} alt={item.alt} />
    </div>
  );
}

function VideoThumb({
  item,
  onOpen,
}: {
  item: MediaItem;
  onOpen: () => void;
}) {
  return (
    <div className={styles.thumbWrap} onClick={onOpen}>
      <video className={styles.thumb} src={item.src} muted preload="metadata" />
      <span className={styles.playBadge}>▶</span>
    </div>
  );
}

// ─── Lightbox ─────────────────────────────────────────────────────

function Lightbox({
  item,
  onClose,
}: {
  item: MediaItem;
  onClose: () => void;
}) {
  // Close on Escape.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  return (
    <div className={styles.lightboxOverlay} onClick={onClose}>
      <div
        className={styles.lightboxContent}
        onClick={(e) => e.stopPropagation()}
      >
        {item.kind === "video" ? (
          <video
            className={styles.lightboxMedia}
            src={item.src}
            controls
            autoPlay
          />
        ) : (
          <img
            className={styles.lightboxMedia}
            src={item.src}
            alt={item.alt}
          />
        )}
        <button className={styles.lightboxClose} onClick={onClose}>
          ✕
        </button>
      </div>
    </div>
  );
}

// ─── Main component ───────────────────────────────────────────────

export default function MediaPreview({ html, messageId }: Props): ReactNode {
  const { cleaned, media } = extractMedia(html);
  const [lightboxIdx, setLightboxIdx] = useState<number | null>(null);

  const openLightbox = (idx: number) => setLightboxIdx(idx);
  const closeLightbox = () => setLightboxIdx(null);

  return (
    <>
      {/* Render remaining text (if any) */}
      {cleaned && (
        <span dangerouslySetInnerHTML={{ __html: cleaned }} />
      )}

      {/* Media thumbnails */}
      {media.length > 0 && (
        <div className={styles.mediaGrid}>
          {media.map((item, i) => {
            const key = `${messageId}-${i}`;
            switch (item.kind) {
              case "gif":
                return (
                  <GifThumb
                    key={key}
                    item={item}
                    id={key}
                    onOpen={() => openLightbox(i)}
                  />
                );
              case "image":
                return (
                  <ImageThumb
                    key={key}
                    item={item}
                    onOpen={() => openLightbox(i)}
                  />
                );
              case "video":
                return (
                  <VideoThumb
                    key={key}
                    item={item}
                    onOpen={() => openLightbox(i)}
                  />
                );
            }
          })}
        </div>
      )}

      {/* Lightbox */}
      {lightboxIdx !== null && media[lightboxIdx] && (
        <Lightbox item={media[lightboxIdx]} onClose={closeLightbox} />
      )}
    </>
  );
}
