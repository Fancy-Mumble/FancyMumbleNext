/**
 * Canvas-based image resize / compress utilities.
 * No external dependencies - uses only the browser canvas API.
 */

/** Load a data-URL (or blob-URL) into an HTMLImageElement. */
export function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("Failed to load image"));
    img.src = src;
  });
}

/**
 * Resize an image so it fits within `maxWidth × maxHeight`, then
 * compress it as JPEG, progressively lowering quality until the
 * result is under `maxBytes`.
 *
 * @returns A data-URL of the resized JPEG.
 */
export async function resizeImage(
  dataUrl: string,
  maxWidth: number,
  maxHeight: number,
  maxBytes = 100_000,
  format: string = "image/jpeg",
): Promise<string> {
  const img = await loadImage(dataUrl);

  let w = img.naturalWidth;
  let h = img.naturalHeight;

  // Scale down to fit within bounds (maintain aspect ratio).
  if (w > maxWidth) {
    h = Math.round(h * (maxWidth / w));
    w = maxWidth;
  }
  if (h > maxHeight) {
    w = Math.round(w * (maxHeight / h));
    h = maxHeight;
  }

  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(img, 0, 0, w, h);

  // Try progressively lower quality until the result fits.
  for (let quality = 0.85; quality >= 0.3; quality -= 0.1) {
    const result = canvas.toDataURL(format, quality);
    // Estimate raw byte count from base64 length.
    const bytes = Math.ceil(
      (result.length - result.indexOf(",") - 1) * 0.75,
    );
    if (bytes <= maxBytes) return result;
  }

  // Last resort - lowest quality.
  return canvas.toDataURL(format, 0.3);
}

/**
 * Crop a region from an image and output it at the target resolution.
 *
 * @param img   Loaded HTMLImageElement
 * @param sx    Source X in *natural image* pixels
 * @param sy    Source Y in *natural image* pixels
 * @param sw    Source width in *natural image* pixels
 * @param sh    Source height in *natural image* pixels
 * @param tw    Target output width  (px)
 * @param th    Target output height (px)
 * @returns     data-URL of the cropped image (JPEG).
 */
export function cropToCanvas(
  img: HTMLImageElement,
  sx: number,
  sy: number,
  sw: number,
  sh: number,
  tw: number,
  th: number,
): string {
  const canvas = document.createElement("canvas");
  canvas.width = tw;
  canvas.height = th;
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(img, sx, sy, sw, sh, 0, 0, tw, th);
  return canvas.toDataURL("image/jpeg", 0.85);
}
