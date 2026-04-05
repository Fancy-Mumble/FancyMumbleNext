/**
 * MarkdownInput - a chat input with live markdown preview.
 *
 * Shows formatting decorations (bold, italic, underline, strikethrough,
 * code) inline while keeping the raw markdown syntax characters visible.
 * The underlying value is always plain-text markdown.
 *
 * Supports keyboard shortcuts: Ctrl+B, Ctrl+I, Ctrl+U.
 */

import {
  useRef,
  useCallback,
  useEffect,
  useState,
  type KeyboardEvent,
  type ClipboardEvent,
  type ReactNode,
} from "react";
import styles from "./MarkdownInput.module.css";

// --- Markdown -> decorated spans -----------------------------------

interface Segment {
  text: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  code?: boolean;
  link?: boolean;
}

/** Regex matching URLs (http, https, ftp) in plain text. */
const URL_RE = /https?:\/\/[^\s<>"'`,;)\]]+|ftp:\/\/[^\s<>"'`,;)\]]+/g;

/**
 * Parse raw markdown text into decorated segments.
 * Handles: **bold**, *italic*, __underline__, ~~strike~~, `code`, URLs
 */
function parseMarkdown(raw: string): Segment[] {
  const segments: Segment[] = [];
  let i = 0;
  let current = "";
  const pushCurrent = (flags?: Partial<Segment>) => {
    if (current) {
      // Split any accumulated plain text to detect URLs within it.
      pushWithUrls(segments, current, flags);
      current = "";
    }
  };

  while (i < raw.length) {
    // `` `code` ``
    if (raw[i] === "`") {
      pushCurrent();
      const end = raw.indexOf("`", i + 1);
      if (end !== -1) {
        segments.push({ text: raw.slice(i, end + 1), code: true });
        i = end + 1;
        continue;
      }
    }

    // **bold**
    if (raw[i] === "*" && raw[i + 1] === "*") {
      pushCurrent();
      const end = raw.indexOf("**", i + 2);
      if (end !== -1) {
        segments.push({ text: raw.slice(i, end + 2), bold: true });
        i = end + 2;
        continue;
      }
    }

    // *italic* (single *)
    if (raw[i] === "*" && raw[i + 1] !== "*") {
      pushCurrent();
      const end = raw.indexOf("*", i + 1);
      if (end !== -1 && raw[end + 1] !== "*") {
        segments.push({ text: raw.slice(i, end + 1), italic: true });
        i = end + 1;
        continue;
      }
    }

    // __underline__
    if (raw[i] === "_" && raw[i + 1] === "_") {
      pushCurrent();
      const end = raw.indexOf("__", i + 2);
      if (end !== -1) {
        segments.push({ text: raw.slice(i, end + 2), underline: true });
        i = end + 2;
        continue;
      }
    }

    // ~~strikethrough~~
    if (raw[i] === "~" && raw[i + 1] === "~") {
      pushCurrent();
      const end = raw.indexOf("~~", i + 2);
      if (end !== -1) {
        segments.push({ text: raw.slice(i, end + 2), strike: true });
        i = end + 2;
        continue;
      }
    }

    current += raw[i];
    i++;
  }
  pushCurrent();
  return segments;
}

/** Push text into segments, splitting out URLs as `link` segments. */
function pushWithUrls(
  segments: Segment[],
  text: string,
  flags?: Partial<Segment>,
): void {
  URL_RE.lastIndex = 0;
  let lastIdx = 0;
  let match: RegExpExecArray | null;
  while ((match = URL_RE.exec(text)) !== null) {
    if (match.index > lastIdx) {
      segments.push({ text: text.slice(lastIdx, match.index), ...flags });
    }
    segments.push({ text: match[0], link: true, ...flags });
    lastIdx = URL_RE.lastIndex;
  }
  if (lastIdx < text.length) {
    segments.push({ text: text.slice(lastIdx), ...flags });
  }
}

/** CSS class for a segment's formatting. */
function getSegmentClass(seg: Segment): string {
  const classes: string[] = [];
  if (seg.bold) classes.push(styles.mdBold);
  if (seg.italic) classes.push(styles.mdItalic);
  if (seg.underline) classes.push(styles.mdUnderline);
  if (seg.strike) classes.push(styles.mdStrike);
  if (seg.code) classes.push(styles.mdCode);
  if (seg.link) classes.push(styles.mdLink);
  return classes.join(" ");
}

/**
 * Render segments with a custom caret and selection highlight.
 *
 * The caret is a blinking vertical line inserted at the correct character
 * position *within* the formatted overlay, so it naturally tracks the real
 * glyph layout (bold chars are wider -> caret shifts accordingly).
 */
function renderFormattedOverlay(
  segments: Segment[],
  selStart: number,
  selEnd: number,
  showCursor: boolean,
): ReactNode[] {
  const nodes: ReactNode[] = [];
  let keyIdx = 0;

  const hasSelection = showCursor && selStart !== selEnd;
  const cursorPos = showCursor && !hasSelection ? selStart : -1;
  const selFrom = hasSelection ? Math.min(selStart, selEnd) : -1;
  const selTo = hasSelection ? Math.max(selStart, selEnd) : -1;

  // Boundary positions where segments must be split.
  const boundaries = new Set<number>();
  if (cursorPos >= 0) boundaries.add(cursorPos);
  if (hasSelection) {
    boundaries.add(selFrom);
    boundaries.add(selTo);
  }

  let charIdx = 0;
  let cursorInserted = cursorPos < 0;

  for (const seg of segments) {
    const segStart = charIdx;
    const segEnd = charIdx + seg.text.length;
    const cls = getSegmentClass(seg);

    // Find split points strictly inside this segment.
    const localSplits = Array.from(boundaries)
      .filter((p) => p > segStart && p < segEnd)
      .map((p) => p - segStart);

    const breaks = [...new Set([0, ...localSplits, seg.text.length])].sort(
      (a, b) => a - b,
    );

    for (let bi = 0; bi < breaks.length - 1; bi++) {
      const from = breaks[bi];
      const to = breaks[bi + 1];
      const text = seg.text.slice(from, to);
      const globalFrom = segStart + from;
      const globalTo = segStart + to;

      // Insert caret before this slice if it starts at the cursor position.
      if (!cursorInserted && globalFrom === cursorPos) {
        nodes.push(<span key="caret" className={styles.caret} />);
        cursorInserted = true;
      }

      if (text) {
        const inSelection =
          hasSelection && globalFrom >= selFrom && globalTo <= selTo;
        const combined = inSelection
          ? `${cls} ${styles.selection}`.trim()
          : cls;
        nodes.push(
          <span key={keyIdx++} className={combined || undefined}>
            {text}
          </span>,
        );
      }
    }

    charIdx = segEnd;
  }

  // Caret at the very end of the text.
  if (!cursorInserted) {
    nodes.push(<span key="caret" className={styles.caret} />);
  }

  return nodes;
}

// --- Markdown -> HTML (for sending) -------------------------------

/** Convert markdown syntax to HTML for the Mumble message body. */
export function markdownToHtml(raw: string): string {
  let html = raw;
  // Escape HTML entities first
  html = html.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  // `code`  -- must come before bold/italic to avoid mis-parsing
  html = html.replace(/`([^`]+)`/g, "<code>$1</code>");
  // **bold**
  html = html.replace(/\*\*(.+?)\*\*/g, "<b>$1</b>");
  // *italic*
  html = html.replace(/(?<!\*)\*(?!\*)(.+?)(?<!\*)\*(?!\*)/g, "<i>$1</i>");
  // __underline__
  html = html.replace(/__(.+?)__/g, "<u>$1</u>");
  // ~~strikethrough~~
  html = html.replace(/~~(.+?)~~/g, "<s>$1</s>");
  // URLs -> clickable links (must run after entity escaping)
  html = html.replace(
    /(https?:\/\/[^\s<>"'`,;)\]]+|ftp:\/\/[^\s<>"'`,;)\]]+)/g,
    '<a href="$1" target="_blank" rel="noopener noreferrer">$1</a>',
  );
  // Newlines -> <br> (must come last so inline formatting is applied first)
  html = html.replaceAll("\n", "<br>");
  return html;
}

// --- Component ----------------------------------------------------

interface MarkdownInputProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
  onPaste?: (e: ClipboardEvent) => void;
  placeholder?: string;
  disabled?: boolean;
}

export default function MarkdownInput({
  value,
  onChange,
  onSubmit,
  onPaste,
  placeholder,
  disabled,
}: Readonly<MarkdownInputProps>) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const [focused, setFocused] = useState(false);
  const [selStart, setSelStart] = useState(0);
  const [selEnd, setSelEnd] = useState(0);
  const [composing, setComposing] = useState(false);

  /** Read the textarea's current selection and push it into state. */
  const syncSelection = useCallback(() => {
    const el = textareaRef.current;
    if (el) {
      setSelStart(el.selectionStart);
      setSelEnd(el.selectionEnd);
    }
  }, []);

  // Sync scroll between textarea and overlay.
  const syncScroll = useCallback(() => {
    if (textareaRef.current && overlayRef.current) {
      overlayRef.current.scrollTop = textareaRef.current.scrollTop;
      overlayRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
  }, []);

  // Auto-resize textarea and wrapper to content (up to max-height).
  useEffect(() => {
    const el = textareaRef.current;
    const wrapper = el?.parentElement;
    if (!el || !wrapper) return;
    // Reset both heights before measuring so scrollHeight reflects actual content,
    // not the previous explicit height (wrapper falls back to CSS min-height).
    wrapper.style.height = "auto";
    el.style.height = "auto";
    const maxHeight = 200;
    const clamped = Math.min(el.scrollHeight, maxHeight);
    el.style.height = `${clamped}px`;
    wrapper.style.height = `${clamped}px`;
  }, [value]);

  /** Wrap selection / insert at cursor with markdown markers. */
  const wrapSelection = useCallback(
    (before: string, after: string) => {
      const el = textareaRef.current;
      if (!el) return;
      const start = el.selectionStart;
      const end = el.selectionEnd;
      const selected = value.slice(start, end);
      const newVal =
        value.slice(0, start) + before + selected + after + value.slice(end);
      onChange(newVal);
      // Restore cursor position after React re-render.
      requestAnimationFrame(() => {
        el.selectionStart = start + before.length;
        el.selectionEnd = end + before.length;
        el.focus();
        syncSelection();
      });
    },
    [value, onChange, syncSelection],
  );

  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      // Submit on Enter (without Shift).
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        onSubmit();
        return;
      }

      // Markdown shortcuts.
      if (e.ctrlKey || e.metaKey) {
        switch (e.key.toLowerCase()) {
          case "b":
            e.preventDefault();
            wrapSelection("**", "**");
            return;
          case "i":
            e.preventDefault();
            wrapSelection("*", "*");
            return;
          case "u":
            e.preventDefault();
            wrapSelection("__", "__");
            return;
        }
      }
    },
    [onSubmit, wrapSelection],
  );

  const segments = parseMarkdown(value);
  const showPlaceholder = !value && !focused;

  return (
    <div className={`${styles.wrapper} ${focused ? styles.focused : ""}`}>
      {/* Overlay: shows decorated text + custom caret + selection */}
      <div ref={overlayRef} className={styles.overlay} aria-hidden>
        {value
          ? renderFormattedOverlay(
              segments,
              selStart,
              selEnd,
              focused && !composing,
            )
          : null}
        {showPlaceholder && (
          <span className={styles.placeholder}>{placeholder}</span>
        )}
      </div>
      {/* Actual editable textarea (fully invisible - input only) */}
      <textarea
        ref={textareaRef}
        className={styles.textarea}
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
          setSelStart(e.target.selectionStart);
          setSelEnd(e.target.selectionEnd);
        }}
        onKeyDown={handleKeyDown}
        onPaste={onPaste}
        onScroll={syncScroll}
        onSelect={syncSelection}
        onCompositionStart={() => setComposing(true)}
        onCompositionEnd={() => setComposing(false)}
        onFocus={() => {
          setFocused(true);
          syncSelection();
        }}
        onBlur={() => setFocused(false)}
        disabled={disabled}
        rows={1}
        spellCheck
      />
    </div>
  );
}
