import { useState, useEffect, useRef, useCallback } from "react";
import { loadImage, cropToCanvas, resizeImage } from "./imageUtils";
import styles from "./SettingsPage.module.css";

interface ImageEditorProps {
  /** Raw image data-URL to edit. */
  src: string;
  /** Circle guide for avatars, rectangle for banners. */
  cropShape: "circle" | "rect";
  /** Desired output width. */
  targetWidth: number;
  /** Desired output height. */
  targetHeight: number;
  /** Maximum byte size of the resulting data-URL payload. */
  maxBytes?: number;
  /** Called with the final resized data-URL. */
  onConfirm: (dataUrl: string) => void;
  /** Called when the user cancels. */
  onCancel: () => void;
}

/** Viewport dimensions for the editor (CSS px). */
const VP_W = 380;
const VP_H = 380;

export function ImageEditor({
  src,
  cropShape,
  targetWidth,
  targetHeight,
  maxBytes = 100_000,
  onConfirm,
  onCancel,
}: ImageEditorProps) {
  const [img, setImg] = useState<HTMLImageElement | null>(null);
  const [zoom, setZoom] = useState(1);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [minZoom, setMinZoom] = useState(0.1);
  const dragging = useRef(false);
  const lastMouse = useRef({ x: 0, y: 0 });

  // Crop-region dimensions inside the viewport.
  const cropW = cropShape === "circle" ? Math.min(VP_W, VP_H) * 0.7 : VP_W * 0.9;
  const cropH =
    cropShape === "circle"
      ? cropW
      : cropW * (targetHeight / targetWidth);
  const cropLeft = (VP_W - cropW) / 2;
  const cropTop = (VP_H - cropH) / 2;

  // Load image and compute initial zoom / position.
  useEffect(() => {
    loadImage(src).then((loaded) => {
      setImg(loaded);
      const iw = loaded.naturalWidth;
      const ih = loaded.naturalHeight;
      // Zoom so the image just covers the crop area.
      const mz = Math.max(cropW / iw, cropH / ih);
      setMinZoom(mz);
      setZoom(mz);
      // Centre the image over the crop area.
      setPos({
        x: cropLeft + cropW / 2 - (iw * mz) / 2,
        y: cropTop + cropH / 2 - (ih * mz) / 2,
      });
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [src]);

  /** Clamp position so the image always covers the crop area. */
  const clamp = useCallback(
    (x: number, y: number, z: number, iw: number, ih: number) => ({
      x: Math.min(cropLeft, Math.max(cropLeft + cropW - iw * z, x)),
      y: Math.min(cropTop, Math.max(cropTop + cropH - ih * z, y)),
    }),
    [cropLeft, cropTop, cropW, cropH],
  );

  const handleZoom = useCallback(
    (newZoom: number) => {
      if (!img) return;
      const z = Math.max(minZoom, Math.min(minZoom * 5, newZoom));
      // Keep crop centre fixed while zooming.
      const cx = cropLeft + cropW / 2;
      const cy = cropTop + cropH / 2;
      const imgCx = (cx - pos.x) / zoom;
      const imgCy = (cy - pos.y) / zoom;
      const np = clamp(cx - imgCx * z, cy - imgCy * z, z, img.naturalWidth, img.naturalHeight);
      setZoom(z);
      setPos(np);
    },
    [img, minZoom, zoom, pos, clamp, cropLeft, cropTop, cropW, cropH],
  );

  // -- Mouse / pointer handlers ----------------------------------
  const onPointerDown = (e: React.PointerEvent) => {
    dragging.current = true;
    lastMouse.current = { x: e.clientX, y: e.clientY };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    if (!dragging.current || !img) return;
    const dx = e.clientX - lastMouse.current.x;
    const dy = e.clientY - lastMouse.current.y;
    lastMouse.current = { x: e.clientX, y: e.clientY };
    setPos((p) => clamp(p.x + dx, p.y + dy, zoom, img.naturalWidth, img.naturalHeight));
  };

  const onPointerUp = () => {
    dragging.current = false;
  };

  const onWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    handleZoom(zoom * (1 - e.deltaY * 0.001));
  };

  // -- Export -----------------------------------------------------
  const handleConfirm = useCallback(async () => {
    if (!img) return;
    // Map crop area back to natural-image coordinates.
    const sx = (cropLeft - pos.x) / zoom;
    const sy = (cropTop - pos.y) / zoom;
    const sw = cropW / zoom;
    const sh = cropH / zoom;
    const raw = cropToCanvas(img, sx, sy, sw, sh, targetWidth, targetHeight);
    const final = await resizeImage(raw, targetWidth, targetHeight, maxBytes);
    onConfirm(final);
  }, [img, pos, zoom, cropLeft, cropTop, cropW, cropH, targetWidth, targetHeight, maxBytes, onConfirm]);

  if (!img) return null;

  return (
    <div className={styles.editorOverlay} onClick={onCancel}>
      <div className={styles.editorModal} onClick={(e) => e.stopPropagation()}>
        <h3 className={styles.editorTitle}>
          {cropShape === "circle" ? "Crop Avatar" : "Crop Banner"}
        </h3>

        {/* Viewport */}
        <div
          className={styles.editorViewport}
          style={{ width: VP_W, height: VP_H }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onWheel={onWheel}
        >
          <img
            src={src}
            alt=""
            draggable={false}
            className={styles.editorImg}
            style={{
              transform: `translate(${pos.x}px, ${pos.y}px) scale(${zoom})`,
              transformOrigin: "0 0",
              width: img.naturalWidth,
              height: img.naturalHeight,
            }}
          />

          {/* SVG mask overlay - darkens everything outside the crop region */}
          <svg className={styles.editorMask} viewBox={`0 0 ${VP_W} ${VP_H}`}>
            <defs>
              <mask id="crop-mask">
                <rect width={VP_W} height={VP_H} fill="white" />
                {cropShape === "circle" ? (
                  <circle
                    cx={VP_W / 2}
                    cy={VP_H / 2}
                    r={cropW / 2}
                    fill="black"
                  />
                ) : (
                  <rect
                    x={cropLeft}
                    y={cropTop}
                    width={cropW}
                    height={cropH}
                    rx={6}
                    fill="black"
                  />
                )}
              </mask>
            </defs>
            <rect
              width={VP_W}
              height={VP_H}
              fill="rgba(0,0,0,0.55)"
              mask="url(#crop-mask)"
            />
            {/* Crop border */}
            {cropShape === "circle" ? (
              <circle
                cx={VP_W / 2}
                cy={VP_H / 2}
                r={cropW / 2}
                fill="none"
                stroke="rgba(255,255,255,0.5)"
                strokeWidth={1.5}
              />
            ) : (
              <rect
                x={cropLeft}
                y={cropTop}
                width={cropW}
                height={cropH}
                rx={6}
                fill="none"
                stroke="rgba(255,255,255,0.5)"
                strokeWidth={1.5}
              />
            )}
          </svg>
        </div>

        {/* Zoom slider */}
        <div className={styles.editorControls}>
          <span className={styles.editorZoomIcon}>🔍</span>
          <input
            type="range"
            min={minZoom}
            max={minZoom * 5}
            step={minZoom * 0.05}
            value={zoom}
            onChange={(e) => handleZoom(Number(e.target.value))}
            className={styles.editorSlider}
          />
        </div>

        {/* Actions */}
        <div className={styles.editorActions}>
          <button
            type="button"
            className={styles.ghostBtn}
            onClick={onCancel}
          >
            Cancel
          </button>
          <button
            type="button"
            className={styles.applyBtn}
            onClick={handleConfirm}
            style={{ padding: "8px 24px", width: "auto" }}
          >
            Apply
          </button>
        </div>
      </div>
    </div>
  );
}
