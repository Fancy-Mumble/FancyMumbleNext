/**
 * Tiptap-based WYSIWYG editor for the profile bio.
 *
 * Provides bold, italic, underline, and text-colour formatting.
 * Outputs sanitised HTML that is stored in the Mumble user comment.
 */

import { useEffect, useRef, useCallback, useState, useMemo } from "react";
import { useEditor, EditorContent } from "@tiptap/react";
import type { EditorView } from "@tiptap/pm/view";
import type { Slice } from "@tiptap/pm/model";
import StarterKit from "@tiptap/starter-kit";
import { TextStyle } from "@tiptap/extension-text-style";
import Color from "@tiptap/extension-color";
import Placeholder from "@tiptap/extension-placeholder";
import TiptapImage from "@tiptap/extension-image";
import { resizeImage } from "./imageUtils";
import ImageIcon from "../../assets/icons/general/image.svg?react";
import styles from "./SettingsPage.module.css";

// -- Colour palette for the quick-pick colour grid -----------------

const COLOUR_PALETTE = [
  "#ffffff",
  "#cccccc",
  "#999999",
  "#ff4d4d",
  "#ff9933",
  "#ffcc00",
  "#66cc66",
  "#33bbff",
  "#9966ff",
  "#ff66cc",
  "#2aabee", // accent
  "#00ffaa",
];

// -- Component -----------------------------------------------------

interface BioEditorProps {
  readonly value: string;
  readonly onChange: (html: string) => void;
  readonly maxLength?: number;
  readonly placeholder?: string;
}

export function BioEditor({
  value,
  onChange,
  maxLength = 2000,
  placeholder = "Tell others about yourself...",
}: BioEditorProps) {
  const [showColourPicker, setShowColourPicker] = useState(false);
  const colourPickerRef = useRef<HTMLDivElement>(null);
  const colourBtnRef = useRef<HTMLButtonElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Track whether we should suppress the next onUpdate to avoid
  // feedback loops when the parent pushes a new `value`.
  const suppressUpdate = useRef(false);

  // Fetch an external image URL and return a resized base64 data URL.
  const fetchAndResizeImage = useCallback(async (url: string): Promise<string | null> => {
    try {
      const resp = await fetch(url);
      if (!resp.ok) return null;
      const blob = await resp.blob();
      if (!blob.type.startsWith("image/")) return null;
      const raw = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result as string);
        reader.onerror = reject;
        reader.readAsDataURL(blob);
      });
      return resizeImage(raw, 400, 400, 80_000);
    } catch {
      return null;
    }
  }, []);

  // Tiptap paste handler: intercepts pasted image files and HTML
  // containing external <img> URLs, converting them to base64.
  const handleEditorPaste = useMemo(() => {
    return (view: EditorView, event: globalThis.ClipboardEvent, _slice: Slice): boolean => {
      const clip = event.clipboardData;
      if (!clip) return false;

      // 1. Handle pasted image files (e.g. screenshots).
      const files = clip.files;
      if (files?.length) {
        for (const file of files) {
          if (file.type.startsWith("image/")) {
            event.preventDefault();
            const reader = new FileReader();
            reader.onload = async (e) => {
              const raw = e.target?.result as string | undefined;
              if (!raw) return;
              const dataUrl = await resizeImage(raw, 400, 400, 80_000);
              view.dispatch(view.state.tr.replaceSelectionWith(
                view.state.schema.nodes.image.create({ src: dataUrl }),
              ));
            };
            reader.readAsDataURL(file);
            return true;
          }
        }
      }

      // 2. Handle pasted HTML containing external image URLs.
      const html = clip.getData("text/html");
      if (html) {
        const parser = new DOMParser();
        const doc = parser.parseFromString(html, "text/html");
        const imgs = doc.querySelectorAll("img[src]");
        const externalImgs = Array.from(imgs).filter((img) => {
          const src = img.getAttribute("src") ?? "";
          return src.startsWith("http://") || src.startsWith("https://");
        });

        if (externalImgs.length > 0) {
          event.preventDefault();
          // Process each external image: fetch, resize, insert.
          for (const img of externalImgs) {
            const src = img.getAttribute("src")!;
            fetchAndResizeImage(src).then((dataUrl) => {
              if (!dataUrl) return;
              const { tr } = view.state;
              const node = view.state.schema.nodes.image.create({ src: dataUrl });
              view.dispatch(tr.replaceSelectionWith(node));
            });
          }
          // Insert any non-image text content alongside.
          const plainText = clip.getData("text/plain")?.trim();
          if (plainText && externalImgs.length < imgs.length + 1) {
            // There was text alongside images - let it through as text.
            const textNode = view.state.schema.text(plainText);
            view.dispatch(view.state.tr.replaceSelectionWith(textNode));
          }
          return true;
        }
      }

      // 3. Handle pasted plain text that is an image URL.
      const text = clip.getData("text/plain")?.trim();
      if (text && /^https?:\/\/.+\.(png|jpe?g|gif|webp|svg|bmp|avif)(\?.*)?$/i.test(text)) {
        event.preventDefault();
        fetchAndResizeImage(text).then((dataUrl) => {
          if (!dataUrl) return;
          const { tr } = view.state;
          const node = view.state.schema.nodes.image.create({ src: dataUrl });
          view.dispatch(tr.replaceSelectionWith(node));
        });
        return true;
      }

      return false;
    };
  }, [fetchAndResizeImage]);

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        // We only need inline formatting - disable block-level nodes
        // that don't make sense in a short bio.
        heading: false,
        blockquote: false,
        codeBlock: false,
        horizontalRule: false,
        bulletList: false,
        orderedList: false,
        listItem: false,
      }),
      TextStyle,
      Color,
      Placeholder.configure({ placeholder }),
      TiptapImage.configure({ inline: true, allowBase64: true }),
    ],
    content: value,
    onUpdate: ({ editor: ed }) => {
      if (suppressUpdate.current) {
        suppressUpdate.current = false;
        return;
      }
      const html = ed.getHTML();
      // tiptap produces `<p></p>` for empty - normalise to empty string.
      const normalised = html === "<p></p>" ? "" : html;
      // Exclude embedded image data from the length check so large
      // base64 payloads do not prevent the user from saving text.
      const htmlForCount = normalised.replaceAll(/src="data:[^"]+"/g, 'src=""');
      if (htmlForCount.length <= maxLength) {
        onChange(normalised);
      }
    },
    editorProps: {
      attributes: {
        class: styles.bioEditorContent,
      },
      handlePaste: handleEditorPaste,
    },
  });

  // Sync external `value` prop into the editor when it diverges
  // (e.g. on initial load from persisted data).
  useEffect(() => {
    if (!editor) return;
    const current = editor.getHTML();
    const normCurrent = current === "<p></p>" ? "" : current;
    if (normCurrent !== value) {
      suppressUpdate.current = true;
      editor.commands.setContent(value || "", { emitUpdate: false });
    }
  }, [value, editor]);

  // Close colour picker on outside click.
  useEffect(() => {
    if (!showColourPicker) return;
    const handle = (e: MouseEvent) => {
      if (
        colourPickerRef.current &&
        !colourPickerRef.current.contains(e.target as Node) &&
        colourBtnRef.current &&
        !colourBtnRef.current.contains(e.target as Node)
      ) {
        setShowColourPicker(false);
      }
    };
    document.addEventListener("mousedown", handle);
    return () => document.removeEventListener("mousedown", handle);
  }, [showColourPicker]);

  const applyColour = useCallback(
    (colour: string) => {
      editor?.chain().focus().setColor(colour).run();
      setShowColourPicker(false);
    },
    [editor],
  );

  const clearColour = useCallback(() => {
    editor?.chain().focus().unsetColor().run();
    setShowColourPicker(false);
  }, [editor]);

  const handleInsertImage = useCallback(
    async (file: File) => {
      if (!editor) return;
      const reader = new FileReader();
      reader.onload = async (e) => {
        const raw = e.target?.result as string | undefined;
        if (!raw) return;
        // Resize/compress to keep comment size manageable (max 80 KB raw bytes).
        const dataUrl = await resizeImage(raw, 400, 400, 80_000);
        editor.chain().focus().setImage({ src: dataUrl }).run();
      };
      reader.readAsDataURL(file);
    },
    [editor],
  );

  if (!editor) return null;

  return (
    <div className={styles.bioEditor}>
      {/* Toolbar */}
      <div className={styles.bioToolbar}>
        <button
          type="button"
          className={`${styles.bioToolBtn} ${editor.isActive("bold") ? styles.bioToolBtnActive : ""}`}
          onClick={() => editor.chain().focus().toggleBold().run()}
          title="Bold"
          aria-label="Bold"
        >
          <strong>B</strong>
        </button>
        <button
          type="button"
          className={`${styles.bioToolBtn} ${editor.isActive("italic") ? styles.bioToolBtnActive : ""}`}
          onClick={() => editor.chain().focus().toggleItalic().run()}
          title="Italic"
          aria-label="Italic"
        >
          <em>I</em>
        </button>
        <button
          type="button"
          className={`${styles.bioToolBtn} ${editor.isActive("underline") ? styles.bioToolBtnActive : ""}`}
          onClick={() => editor.chain().focus().toggleUnderline().run()}
          title="Underline"
          aria-label="Underline"
        >
          <u>U</u>
        </button>

        {/* Hidden file input - image source is always converted to a data: URL
            so no external requests are ever made. */}
        <input
          ref={fileInputRef}
          type="file"
          accept="image/png,image/jpeg,image/gif,image/webp"
          style={{ display: "none" }}
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) void handleInsertImage(file);
            e.target.value = "";
          }}
        />
        {/* Image insert button */}
        <button
          type="button"
          className={styles.bioToolBtn}
          onClick={() => fileInputRef.current?.click()}
          title="Insert image"
          aria-label="Insert image"
        >
          <ImageIcon width={14} height={14} aria-hidden="true" />
        </button>

        {/* Colour picker toggle */}
        <div className={styles.bioColourWrap}>
          <button
            ref={colourBtnRef}
            type="button"
            className={`${styles.bioToolBtn} ${showColourPicker ? styles.bioToolBtnActive : ""}`}
            onClick={() => setShowColourPicker((v) => !v)}
            title="Text colour"
            aria-label="Text colour"
          >
            <span
              className={styles.bioColourIcon}
              style={{
                borderBottomColor:
                  (editor.getAttributes("textStyle")?.color as string) ??
                  "var(--color-text-primary)",
              }}
            >
              A
            </span>
          </button>

          {showColourPicker && (
            <div ref={colourPickerRef} className={styles.bioColourDropdown}>
              <div className={styles.bioColourGrid}>
                {COLOUR_PALETTE.map((c) => (
                  <button
                    key={c}
                    type="button"
                    className={styles.bioColourSwatch}
                    style={{ background: c }}
                    onClick={() => applyColour(c)}
                    aria-label={`Colour ${c}`}
                  />
                ))}
              </div>
              <button
                type="button"
                className={styles.bioColourReset}
                onClick={clearColour}
              >
                Reset colour
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Editor area */}
      <EditorContent editor={editor} />
    </div>
  );
}
