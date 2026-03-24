/**
 * Tiptap-based WYSIWYG editor for the profile bio.
 *
 * Provides bold, italic, underline, and text-colour formatting.
 * Outputs sanitised HTML that is stored in the Mumble user comment.
 */

import { useEffect, useRef, useCallback, useState } from "react";
import { useEditor, EditorContent } from "@tiptap/react";
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
