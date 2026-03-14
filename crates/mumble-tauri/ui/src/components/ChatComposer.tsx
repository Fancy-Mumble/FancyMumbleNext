import { useState, useRef, useCallback, type ClipboardEvent } from "react";
import MarkdownInput from "./MarkdownInput";
import GifPicker from "./GifPicker";
import PollCreator from "./PollCreator";
import styles from "./ChatView.module.css";
import { isMobilePlatform } from "../utils/platform";

interface ChatComposerProps {
  readonly draft: string;
  readonly onChange: (value: string) => void;
  readonly onSend: () => void;
  readonly onPaste: (e: ClipboardEvent) => void;
  readonly onFileSelected: (file: File) => Promise<void>;
  readonly onGifSelect: (url: string, alt: string) => Promise<void>;
  readonly onPollCreate: (question: string, options: string[], multiple: boolean) => Promise<void>;
  readonly disabled?: boolean;
}

export default function ChatComposer({
  draft,
  onChange,
  onSend,
  onPaste,
  onFileSelected,
  onGifSelect,
  onPollCreate,
  disabled = false,
}: ChatComposerProps) {
  const [showGifPicker, setShowGifPicker] = useState(false);
  const [showPollCreator, setShowPollCreator] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleAttach = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleFileChange = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;
      e.target.value = "";
      await onFileSelected(file);
    },
    [onFileSelected],
  );

  const isMobile = isMobilePlatform();

  return (
    <div className={styles.composerWrapper}>
      {showGifPicker && (
        <GifPicker
          onSelect={onGifSelect}
          onClose={() => setShowGifPicker(false)}
        />
      )}
      {showPollCreator && (
        <PollCreator
          onSubmit={onPollCreate}
          onClose={() => setShowPollCreator(false)}
        />
      )}
      <div className={styles.composer}>
        <input
          ref={fileInputRef}
          type="file"
          accept="image/*,video/*"
          className={styles.hiddenFileInput}
          onChange={handleFileChange}
        />

        {/* Attach button */}
        <button
          className={styles.attachBtn}
          onClick={handleAttach}
          disabled={disabled}
          title="Attach image, GIF, or video"
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M21.44 11.05l-9.19 9.19a6 6 0 01-8.49-8.49l9.19-9.19a4 4 0 015.66 5.66l-9.2 9.19a2 2 0 01-2.83-2.83l8.49-8.48" />
          </svg>
        </button>

        {/* GIF button */}
        <button
          className={`${styles.attachBtn} ${showGifPicker ? styles.attachBtnActive : ""}`}
          onClick={() => setShowGifPicker((s) => !s)}
          disabled={disabled}
          title="GIF picker"
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <rect x="2" y="2" width="20" height="20" rx="5" />
            <text x="12" y="16" textAnchor="middle" fill="currentColor" stroke="none" fontSize="10" fontWeight="bold">GIF</text>
          </svg>
        </button>

        {/* Poll button */}
        <button
          className={styles.attachBtn}
          onClick={() => setShowPollCreator(true)}
          disabled={disabled}
          title="Create a poll"
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" stroke="none">
            <path d="M3 3h4v18H3V3zm7 4h4v14h-4V7zm7 4h4v10h-4V11z" />
          </svg>
        </button>

        <MarkdownInput
          value={draft}
          onChange={onChange}
          onSubmit={onSend}
          onPaste={onPaste}
          placeholder={isMobile ? "Write a message…" : "Write a message… (Ctrl+B/I/U for formatting)"}
          disabled={disabled}
        />

        <button
          className={styles.sendBtn}
          onClick={onSend}
          disabled={!draft.trim() || disabled}
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
            <path d="M2.01 21L23 12 2.01 3 2 10l15 2-15 2z" />
          </svg>
        </button>
      </div>
    </div>
  );
}
