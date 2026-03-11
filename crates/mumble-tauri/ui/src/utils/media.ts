/**
 * Utilities for encoding media files as base64 HTML for Mumble messages.
 *
 * Mumble text messages are HTML.  Images/videos are embedded as
 * `<img src="data:...;base64,...">` or `<video>` tags with base64 data URLs.
 * The server enforces `image_message_length` as the max byte length.
 */

/** Detected media type from file. */
export type MediaKind = "image" | "gif" | "video";

/** Detect kind from MIME type. */
export function mediaKind(mime: string): MediaKind | null {
  if (mime === "image/gif") return "gif";
  if (mime.startsWith("image/")) return "image";
  if (mime.startsWith("video/")) return "video";
  return null;
}

/**
 * Read a File as a base64 data-URL string.
 * Returns the full `data:<mime>;base64,...` string.
 */
export function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

/**
 * Compress / downscale an image so its base64 data-URL fits within
 * `maxBytes`.  Returns the data-URL string.
 *
 * Strategy - maximize visual quality:
 *   1. Original fits → return untouched (lossless).
 *   2. Re-encode as JPEG at full resolution.
 *      Binary-search quality 0.1–0.95 to find the highest quality
 *      that fits.  Keeping original pixel dimensions is almost always
 *      preferable over scaling down.
 *   3. Only if even quality 0.1 at full resolution overflows, scale
 *      down.  Estimate a starting scale from the size ratio, then
 *      binary-search to maximize dimensions.  Finally sweep quality
 *      upward at the chosen scale to use any remaining budget.
 */
export async function fitImage(
  file: File,
  maxBytes: number,
): Promise<string> {
  if (maxBytes < 5000) maxBytes = 131072; // guard against bogus limits

  const dataUrl = await fileToDataUrl(file);

  // 1. Original fits → return as-is.
  if (dataUrl.length <= maxBytes) return dataUrl;

  const img = await loadImage(dataUrl);
  const srcW = img.naturalWidth || img.width;
  const srcH = img.naturalHeight || img.height;
  if (srcW === 0 || srcH === 0) throw new Error("Image has zero dimensions");

  // Leave room for the HTML wrapper (<img src="..." alt="..." />)
  const budget = maxBytes - 100;

  /** Render at `scale` × original dimensions with given JPEG `quality`. */
  async function tryEncode(scale: number, quality: number): Promise<string> {
    const w = Math.max(1, Math.round(srcW * scale));
    const h = Math.max(1, Math.round(srcH * scale));
    const canvas = new OffscreenCanvas(w, h);
    const ctx = canvas.getContext("2d");
    ctx?.drawImage(img, 0, 0, w, h);
    const blob = await canvas.convertToBlob({ type: "image/jpeg", quality });
    return blobToDataUrl(blob);
  }

  /** Binary-search quality at a fixed `scale` to maximize it. */
  async function bestQualityAt(
    scale: number,
    qMin: number,
    qMax: number,
    iterations: number,
  ): Promise<string | null> {
    // Verify that qMin actually fits.
    let best = await tryEncode(scale, qMin);
    if (best.length > budget) return null;

    let lo = qMin,
      hi = qMax;
    for (let i = 0; i < iterations; i++) {
      const mid = (lo + hi) / 2;
      const r = await tryEncode(scale, mid);
      if (r.length <= budget) {
        best = r;
        lo = mid;
      } else {
        hi = mid;
      }
    }
    return best;
  }

  // 2. Try full resolution - binary-search quality.
  const fullRes = await bestQualityAt(1, 0.1, 0.95, 8);
  if (fullRes) return fullRes;

  // 3. Full resolution doesn't fit even at q=0.1 → scale down.
  //    Estimate starting scale from the byte-size ratio.
  const lowQ = await tryEncode(1, 0.1);
  const estScale = Math.min(0.95, Math.sqrt(budget / lowQ.length) * 1.1);

  //    Find a guaranteed-to-fit lower bound by halving from the estimate.
  let lo = 0;
  let bestScaled: string | null = null;
  let probe = estScale;
  for (let i = 0; i < 15 && probe >= 0.005; i++) {
    const r = await tryEncode(probe, 0.7);
    if (r.length <= budget) {
      bestScaled = r;
      lo = probe;
      break;
    }
    probe *= 0.5;
  }
  if (!bestScaled) return tryEncode(0.01, 0.1); // absolute fallback

  //    Binary-search scale upward from lo.
  let hi = Math.min(1, lo * 3);
  for (let i = 0; i < 12; i++) {
    if (hi - lo < 0.002) break;
    const mid = (lo + hi) / 2;
    const r = await tryEncode(mid, 0.7);
    if (r.length <= budget) {
      bestScaled = r;
      lo = mid;
    } else {
      hi = mid;
    }
  }

  //    Maximize quality at the final scale.
  const finalQ = await bestQualityAt(lo, 0.7, 0.95, 6);
  return finalQ ?? bestScaled;
}

/**
 * Build the HTML string to embed a media file in a Mumble text message.
 */
export function mediaToHtml(
  dataUrl: string,
  kind: MediaKind,
  fileName: string,
): string {
  switch (kind) {
    case "image":
    case "gif":
      return `<img src="${dataUrl}" alt="${escapeAttr(fileName)}" />`;
    case "video":
      return `<video src="${dataUrl}" controls>${escapeAttr(fileName)}</video>`;
  }
}

/**
 * Compress a video to fit within `maxBytes`.
 *
 * If the raw file already fits, returns it as-is.  Otherwise extracts a
 * poster frame and compresses it as a JPEG image via `fitImage`.
 *
 * Returns `{ dataUrl, kind }` - `kind` will be `"video"` when the
 * original was kept, or `"image"` when a still frame was extracted.
 */
export async function fitVideo(
  file: File,
  maxBytes: number,
): Promise<{ dataUrl: string; kind: MediaKind }> {
  const dataUrl = await fileToDataUrl(file);

  // If the raw video fits, return as-is.
  if (dataUrl.length <= maxBytes) {
    return { dataUrl, kind: "video" };
  }

  // Video is far too large for the Mumble limit - extract a poster
  // frame and compress it as JPEG so the recipient sees something.
  console.log(
    `[fitVideo] video too large (${dataUrl.length} > ${maxBytes}), extracting poster frame`,
  );

  const frameBlob = await extractVideoFrame(file);
  const frameFile = new File([frameBlob], "frame.jpg", {
    type: "image/jpeg",
  });
  const frameDataUrl = await fitImage(frameFile, maxBytes);
  return { dataUrl: frameDataUrl, kind: "image" };
}

// ─── Helpers ──────────────────────────────────────────────────────

/**
 * Extract a representative frame from a video file as a JPEG blob.
 * Seeks to 1 s or 25 % of duration (whichever is smaller).
 */
async function extractVideoFrame(file: File): Promise<Blob> {
  const url = URL.createObjectURL(file);
  try {
    const video = document.createElement("video");
    video.muted = true;
    video.preload = "auto";

    await new Promise<void>((resolve, reject) => {
      video.onloadeddata = () => resolve();
      video.onerror = () => reject(new Error("Failed to load video"));
      video.src = url;
    });

    // Seek to a representative position.
    const seekTo = Math.min(1, video.duration * 0.25);
    if (seekTo > 0) {
      await new Promise<void>((resolve) => {
        video.onseeked = () => resolve();
        video.currentTime = seekTo;
      });
    }

    const w = video.videoWidth || 1;
    const h = video.videoHeight || 1;
    const canvas = new OffscreenCanvas(w, h);
    const ctx = canvas.getContext("2d");
    ctx?.drawImage(video, 0, 0, w, h);

    return canvas.convertToBlob({ type: "image/jpeg", quality: 0.92 });
  } finally {
    URL.revokeObjectURL(url);
  }
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("Failed to load image"));
    img.src = src;
  });
}

function blobToDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

function escapeAttr(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}
