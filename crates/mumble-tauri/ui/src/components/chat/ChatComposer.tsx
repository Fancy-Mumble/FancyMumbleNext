import { useState, useRef, useCallback, type ClipboardEvent } from "react";
import MarkdownInput from "./MarkdownInput";
import GifPicker from "./GifPicker";
import styles from "./ChatView.module.css";
import AttachIcon from "../../assets/icons/action/attach.svg?react";
import GifIcon from "../../assets/icons/communication/gif.svg?react";
import SendIcon from "../../assets/icons/action/send.svg?react";
import { isMobile } from "../../utils/platform";

interface ChatComposerProps {
  readonly draft: string;
  readonly onChange: (value: string) => void;
  readonly onSend: () => void;
  readonly onPaste: (e: ClipboardEvent) => void;
  readonly onFileSelected: (file: File) => Promise<void>;
  readonly onGifSelect: (url: string, alt: string) => Promise<void>;
  readonly disabled?: boolean;
  readonly hasPendingQuotes?: boolean;
}

export default function ChatComposer({
  draft,
  onChange,
  onSend,
  onPaste,
  onFileSelected,
  onGifSelect,
  disabled = false,
  hasPendingQuotes = false,
}: ChatComposerProps) {
  const [showGifPicker, setShowGifPicker] = useState(false);
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

  return (
    <div className={styles.composerWrapper}>
      {showGifPicker && (
        <GifPicker
          onSelect={onGifSelect}
          onClose={() => setShowGifPicker(false)}
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
          <AttachIcon width={20} height={20} />
        </button>

        {/* GIF button */}
        <button
          className={`${styles.attachBtn} ${showGifPicker ? styles.attachBtnActive : ""}`}
          onClick={() => setShowGifPicker((s) => !s)}
          disabled={disabled}
          title="GIF picker"
        >
          <GifIcon width={20} height={20} />
        </button>

        <MarkdownInput
          value={draft}
          onChange={onChange}
          onSubmit={onSend}
          onPaste={onPaste}
          placeholder={isMobile ? "Write a message..." : "Write a message... (Ctrl+B/I/U for formatting)"}
          disabled={disabled}
        />

        <button
          className={styles.sendBtn}
          onClick={onSend}
          disabled={(!draft.trim() && !hasPendingQuotes) || disabled}
        >
          <SendIcon width={20} height={20} />
        </button>
      </div>
    </div>
  );
}
