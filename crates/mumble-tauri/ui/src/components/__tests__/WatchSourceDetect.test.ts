/**
 * Unit tests for `detectVideoSource`.
 */

import { describe, it, expect } from "vitest";
import { detectVideoSource } from "../../utils/watchSourceDetect";

describe("detectVideoSource", () => {
  it("detects a YouTube watch URL", () => {
    const r = detectVideoSource("Check this out https://www.youtube.com/watch?v=dQw4w9WgXcQ", true);
    expect(r).not.toBeNull();
    expect(r?.kind).toBe("youtube");
    expect(r?.youtubeId).toBe("dQw4w9WgXcQ");
  });

  it("detects a YouTube shorts URL", () => {
    const r = detectVideoSource("look https://youtube.com/shorts/abcdefghijk", true);
    expect(r?.kind).toBe("youtube");
    expect(r?.youtubeId).toBe("abcdefghijk");
  });

  it("detects a youtu.be short URL", () => {
    const r = detectVideoSource("https://youtu.be/12345678901", true);
    expect(r?.kind).toBe("youtube");
    expect(r?.youtubeId).toBe("12345678901");
  });

  it("returns null for YouTube when external embeds are disabled", () => {
    const r = detectVideoSource("https://youtu.be/12345678901", false);
    expect(r).toBeNull();
  });

  it("detects a direct video URL", () => {
    const r = detectVideoSource("https://example.com/file.mp4", false);
    expect(r?.kind).toBe("directMedia");
    expect(r?.url).toBe("https://example.com/file.mp4");
  });

  it("decodes HTML entities before matching", () => {
    const r = detectVideoSource(
      'see <a href="https://www.youtube.com/watch?v=dQw4w9WgXcQ&amp;t=10s">link</a>',
      true,
    );
    expect(r?.kind).toBe("youtube");
  });

  it("prefers YouTube over direct media when both are present", () => {
    const r = detectVideoSource(
      "https://example.com/file.mp4 https://youtu.be/12345678901",
      true,
    );
    expect(r?.kind).toBe("youtube");
  });

  it("returns null when nothing matches", () => {
    expect(detectVideoSource("hello world", true)).toBeNull();
  });
});
