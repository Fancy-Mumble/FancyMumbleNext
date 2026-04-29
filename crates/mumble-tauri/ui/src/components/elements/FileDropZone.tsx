import { useRef, useState, useCallback } from "react";
import type { DragEvent, ReactNode } from "react";
import styles from "./FileDropZone.module.css";

interface FileDropZoneProps {
  /** MIME types for the hidden file input. */
  accept: string;
  /** Called when the user picks or drops a file. */
  onFile: (file: File) => void;
  /** Optional preview content (image thumbnail, etc.). */
  preview?: ReactNode;
  /** Label shown in the empty state and during drag. */
  label?: string;
  /** Show a remove button at the bottom. */
  onRemove?: () => void;
  /** Shape of the drop zone. Defaults to "rect". */
  shape?: "rect" | "circle";
  /** Size preset. Defaults to "default". */
  size?: "default" | "small";
}

export function FileDropZone({
  accept,
  onFile,
  preview,
  label = "Drop a file here or click to browse",
  onRemove,
  shape = "rect",
  size = "default",
}: Readonly<FileDropZoneProps>) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [dragging, setDragging] = useState(false);
  const dragCounter = useRef(0);

  const handleDragEnter = useCallback((e: DragEvent) => {
    e.preventDefault();
    dragCounter.current += 1;
    if (dragCounter.current === 1) setDragging(true);
  }, []);

  const handleDragLeave = useCallback((e: DragEvent) => {
    e.preventDefault();
    dragCounter.current -= 1;
    if (dragCounter.current === 0) setDragging(false);
  }, []);

  const handleDragOver = useCallback((e: DragEvent) => {
    e.preventDefault();
  }, []);

  const handleDrop = useCallback(
    (e: DragEvent) => {
      e.preventDefault();
      dragCounter.current = 0;
      setDragging(false);
      const file = e.dataTransfer.files[0];
      if (file) onFile(file);
    },
    [onFile],
  );

  const handleInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (file) onFile(file);
      e.target.value = "";
    },
    [onFile],
  );

  return (
    <div className={`${styles.wrapper} ${shape === "circle" ? styles.wrapperCircle : ""}`}>
      <button
        type="button"
        className={`${styles.zone} ${dragging ? styles.zoneDragging : ""} ${preview ? styles.zoneHasPreview : ""} ${shape === "circle" ? styles.zoneCircle : ""} ${size === "small" ? styles.zoneSmall : ""}`}
        onClick={() => inputRef.current?.click()}
        onDragEnter={handleDragEnter}
        onDragLeave={handleDragLeave}
        onDragOver={handleDragOver}
        onDrop={handleDrop}
      >
        {preview ?? (
          <span className={styles.placeholder}>{label}</span>
        )}
        {dragging && (
          <span className={styles.overlay}>Drop file here</span>
        )}
      </button>
      <input
        ref={inputRef}
        type="file"
        accept={accept}
        hidden
        onChange={handleInputChange}
      />
      {onRemove && (
        <button type="button" className={styles.removeBtn} onClick={onRemove}>
          Remove
        </button>
      )}
    </div>
  );
}
