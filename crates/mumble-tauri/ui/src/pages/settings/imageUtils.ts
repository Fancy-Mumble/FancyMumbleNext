/**
 * Canvas-based image resize / compress utilities.
 * No external dependencies - uses only the browser canvas API.
 */

/** Detect the MIME type from a data-URL prefix. Returns `"image/png"` for
 *  PNG sources (which may have transparency), `"image/jpeg"` otherwise.  */
function detectFormat(dataUrl: string): string {
  if (dataUrl.startsWith("data:image/png")) return "image/png";
  return "image/jpeg";
}

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
 * Resize an image so it fits within `maxWidth x maxHeight`, then
 * compress it, progressively lowering quality until the result is
 * under `maxBytes`.
 *
 * The output format is auto-detected from the source data-URL:
 * PNG sources stay PNG (preserving transparency), everything else
 * becomes JPEG.  Pass an explicit `format` to override.
 *
 * @returns A data-URL of the resized image.
 */
export async function resizeImage(
  dataUrl: string,
  maxWidth: number,
  maxHeight: number,
  maxBytes = 100_000,
  format?: string,
): Promise<string> {
  const outFormat = format ?? detectFormat(dataUrl);
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
    const result = canvas.toDataURL(outFormat, quality);
    // Estimate raw byte count from base64 length.
    const bytes = Math.ceil(
      (result.length - result.indexOf(",") - 1) * 0.75,
    );
    if (bytes <= maxBytes) return result;
  }

  // Last resort - lowest quality.
  return canvas.toDataURL(outFormat, 0.3);
}

/**
 * Crop a region from an image and output it at the target resolution.
 *
 * @param img    Loaded HTMLImageElement
 * @param sx     Source X in *natural image* pixels
 * @param sy     Source Y in *natural image* pixels
 * @param sw     Source width in *natural image* pixels
 * @param sh     Source height in *natural image* pixels
 * @param tw     Target output width  (px)
 * @param th     Target output height (px)
 * @param format Output MIME type (default `"image/jpeg"`).
 * @returns      data-URL of the cropped image.
 */
export function cropToCanvas(
  img: HTMLImageElement,
  sx: number,
  sy: number,
  sw: number,
  sh: number,
  tw: number,
  th: number,
  format = "image/jpeg",
): string {
  const canvas = document.createElement("canvas");
  canvas.width = tw;
  canvas.height = th;
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(img, sx, sy, sw, sh, 0, 0, tw, th);
  return canvas.toDataURL(format, 0.85);
}
