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
  useMemo,
  type KeyboardEvent,
  type ClipboardEvent,
  type ReactNode,
} from "react";
import hljs from "highlight.js/lib/common";
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
  spoiler?: boolean;
  /** Global CSS class from hljs for syntax-highlighted code tokens. */
  hljsClass?: string;
  /** Marker set by parseMarkdown; expanded to hljs tokens by expandFenceSegments. */
  fenceCode?: { lang: string; body: string };
}

/** Regex matching URLs (http, https, ftp) in plain text. */
const URL_RE = /https?:\/\/[^\s<>"'`,)\]]+|ftp:\/\/[^\s<>"'`,)\]]+/g;

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
    // ``` fenced code block (must be checked before single backtick) ```
    if (raw[i] === "`" && raw[i + 1] === "`" && raw[i + 2] === "`") {
      const lineEnd = raw.indexOf("\n", i + 3);
      if (lineEnd !== -1) {
        const lang = raw.slice(i + 3, lineEnd);
        const closeIdx = raw.indexOf("\n```", lineEnd);
        // Accept both closed blocks and unclosed blocks (still being typed).
        const body =
          closeIdx !== -1
            ? raw.slice(lineEnd + 1, closeIdx)
            : raw.slice(lineEnd + 1);
        const fullText =
          closeIdx !== -1
            ? raw.slice(i, closeIdx + 4)
            : raw.slice(i);
        pushCurrent();
        segments.push({ text: fullText, fenceCode: { lang, body } });
        i = closeIdx !== -1 ? closeIdx + 4 : raw.length;
        continue;
      }
    }

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

    // ||spoiler||
    if (raw[i] === "|" && raw[i + 1] === "|") {
      pushCurrent();
      const end = raw.indexOf("||", i + 2);
      if (end !== -1) {
        segments.push({ text: raw.slice(i, end + 2), spoiler: true });
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

/**
 * Walk an hljs-produced HTML fragment and return flat `{text, cls}` tokens.
 * Inherits the nearest ancestor's class name for each text leaf.
 */
function flattenHljs(html: string): Array<{ text: string; cls: string }> {
  const container = document.createElement("div");
  container.innerHTML = html;
  const tokens: Array<{ text: string; cls: string }> = [];

  function visit(node: Node, cls: string): void {
    if (node.nodeType === Node.TEXT_NODE) {
      const t = node.textContent ?? "";
      if (t) tokens.push({ text: t, cls });
    } else if (node.nodeType === Node.ELEMENT_NODE) {
      const el = node as HTMLElement;
      const childCls = el.className || cls;
      for (const child of el.childNodes) visit(child, childCls);
    }
  }

  for (const child of container.childNodes) visit(child, "");
  return tokens;
}

/**
 * Expand any `fenceCode` segments into hljs-coloured sub-segments.
 * All other segments pass through unchanged. Called once per value change
 * via useMemo so hljs runs only on edit, not on every cursor move.
 */
function expandFenceSegments(segments: Segment[]): Segment[] {
  const result: Segment[] = [];
  for (const seg of segments) {
    if (!seg.fenceCode) {
      result.push(seg);
      continue;
    }
    const { lang, body } = seg.fenceCode;
    result.push({ text: `\`\`\`${lang}\n` });
    let tokens: Array<{ text: string; cls: string }>;
    try {
      const hl =
        lang && hljs.getLanguage(lang)
          ? hljs.highlight(body, { language: lang, ignoreIllegals: true })
          : hljs.highlightAuto(body);
      tokens = flattenHljs(hl.value);
    } catch {
      tokens = [{ text: body, cls: "" }];
    }
    for (const t of tokens) {
      result.push({ text: t.text, hljsClass: t.cls || undefined });
    }
    // Only emit the closing fence marker when it was actually present in the raw text.
    if (seg.text.endsWith("\n\`\`\`")) {
      result.push({ text: "\n\`\`\`" });
    }
  }
  return result;
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
  if (seg.spoiler) classes.push(styles.mdSpoiler);
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
        const hlCls = seg.hljsClass ?? "";
        const base = [cls, hlCls].filter(Boolean).join(" ");
        const combined = inSelection ? `${base} ${styles.selection}`.trim() : base;
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

  // Extract fenced code blocks first so their contents are not subject to
  // any further markdown processing (in particular the trailing newline -> <br>
  // pass would otherwise corrupt them and break syntax highlighting).
  const fenceStash: string[] = [];
  html = html.replace(
    /```([a-zA-Z0-9_+-]*)\n([\s\S]*?)```/g,
    (_match, lang: string, body: string) => {
      const cls = lang ? ` class="language-${lang}"` : "";
      const trimmed = body.replace(/\n$/, "");
      fenceStash.push(`<pre><code${cls}>${trimmed}</code></pre>`);
      return `\u0000FENCE${fenceStash.length - 1}\u0000`;
    },
  );

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
  // ||spoiler||
  html = html.replace(/\|\|(.+?)\|\|/g, '<span class="spoiler">$1</span>');
  // URLs -> clickable links (must run after entity escaping)
  html = html.replace(
    /(https?:\/\/[^\s<>"'`,)\]]+|ftp:\/\/[^\s<>"'`,)\]]+)/g,
    '<a href="$1" target="_blank" rel="noopener noreferrer">$1</a>',
  );
  // Newlines -> <br> (must come last so inline formatting is applied first)
  html = html.replaceAll("\n", "<br>");

  // Restore fenced code blocks after the <br> pass so their newlines survive.
  html = html.replace(/\u0000FENCE(\d+)\u0000/g, (_m, idx: string) =>
    fenceStash[Number(idx)] ?? "",
  );
  return html;
}

/** Reverse of markdownToHtml: convert stored HTML back to editable markdown text. */
export function htmlToMarkdown(html: string): string {
  let text = html;
  text = text.replaceAll(/<br\s*\/?>/gi, "\n");
  text = text.replaceAll(
    /<pre><code(?:\s+class="language-([a-zA-Z0-9_+-]+)")?>([\s\S]*?)<\/code><\/pre>/gi,
    (_match, lang: string | undefined, body: string) =>
      `\`\`\`${lang ?? ""}\n${body}\n\`\`\``,
  );
  text = text.replaceAll(/<a[^>]*>([^<]*)<\/a>/gi, "$1");
  text = text.replaceAll(/<code>([^<]*)<\/code>/gi, "`$1`");
  text = text.replaceAll(/<b>([^<]*)<\/b>/gi, "**$1**");
  text = text.replaceAll(/<strong>([^<]*)<\/strong>/gi, "**$1**");
  text = text.replaceAll(/<i>([^<]*)<\/i>/gi, "*$1*");
  text = text.replaceAll(/<em>([^<]*)<\/em>/gi, "*$1*");
  text = text.replaceAll(/<u>([^<]*)<\/u>/gi, "__$1__");
  text = text.replaceAll(/<s>([^<]*)<\/s>/gi, "~~$1~~");
  text = text.replaceAll(
    /<span\s+class="spoiler"[^>]*>([^<]*)<\/span>/gi,
    "||$1||",
  );
  text = text.replaceAll(/<!--[\s\S]*?-->/g, "");
  text = text.replaceAll(/<[^>]*>/g, "");
  text = text.replaceAll("&lt;", "<");
  text = text.replaceAll("&gt;", ">");
  text = text.replaceAll("&amp;", "&");
  return text;
}

// --- Component ----------------------------------------------------

interface MarkdownInputProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
  onPaste?: (e: ClipboardEvent) => void;
  placeholder?: string;
  disabled?: boolean;
  /** Notified whenever the textarea selection changes. */
  onSelectionChange?: (start: number, end: number) => void;
  /** Optional intercept for keystrokes - return true to consume. */
  onKeyDownCapture?: (e: KeyboardEvent<HTMLTextAreaElement>) => boolean;
  /** Imperative API ref for parent-driven text edits (autocomplete, etc.). */
  apiRef?: React.RefObject<MarkdownInputApi | null>;
}

/** Imperative methods exposed to a parent via `apiRef`. */
export interface MarkdownInputApi {
  /** Replace the substring [start, end) with `text` and place caret after it. */
  replaceRange(start: number, end: number, text: string): void;
  /** Focus the underlying textarea. */
  focus(): void;
}

export default function MarkdownInput({
  value,
  onChange,
  onSubmit,
  onPaste,
  placeholder,
  disabled,
  onSelectionChange,
  onKeyDownCapture,
  apiRef,
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
      onSelectionChange?.(el.selectionStart, el.selectionEnd);
    }
  }, [onSelectionChange]);

  // Wire the imperative API exposed to the parent.
  useEffect(() => {
    if (!apiRef) return;
    apiRef.current = {
      replaceRange(start, end, text) {
        const el = textareaRef.current;
        if (!el) return;
        const newVal = value.slice(0, start) + text + value.slice(end);
        const caret = start + text.length;
        onChange(newVal);
        requestAnimationFrame(() => {
          el.focus();
          el.selectionStart = caret;
          el.selectionEnd = caret;
          setSelStart(caret);
          setSelEnd(caret);
          onSelectionChange?.(caret, caret);
        });
      },
      focus() {
        textareaRef.current?.focus();
      },
    };
    return () => {
      if (apiRef.current) apiRef.current = null;
    };
  }, [apiRef, value, onChange, onSelectionChange]);

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
      // Allow the parent to intercept keys (e.g. mention popup navigation).
      if (onKeyDownCapture?.(e)) {
        return;
      }

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
        // Ctrl/Cmd+Shift+H -> spoiler (H for "hide")
        if (e.shiftKey && e.key.toLowerCase() === "h") {
          e.preventDefault();
          wrapSelection("||", "||");
          return;
        }
      }
    },
    [onSubmit, wrapSelection, onKeyDownCapture],
  );

  const segments = useMemo(() => expandFenceSegments(parseMarkdown(value)), [value]);
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
          onSelectionChange?.(e.target.selectionStart, e.target.selectionEnd);
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
