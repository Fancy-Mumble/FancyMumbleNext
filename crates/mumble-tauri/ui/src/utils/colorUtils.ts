/** HSL colour representation (h: 0-360, s: 0-100, l: 0-100). */
export interface HSL {
  h: number;
  s: number;
  l: number;
}

export function hexToHsl(hex: string): HSL {
  const raw = hex.replace("#", "");
  const r = parseInt(raw.substring(0, 2), 16) / 255;
  const g = parseInt(raw.substring(2, 4), 16) / 255;
  const b = parseInt(raw.substring(4, 6), 16) / 255;

  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0;
  let s = 0;

  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
    else if (max === g) h = ((b - r) / d + 2) / 6;
    else h = ((r - g) / d + 4) / 6;
  }

  return { h: Math.round(h * 360), s: Math.round(s * 100), l: Math.round(l * 100) };
}

export function hslToHex(hsl: HSL): string {
  const h = hsl.h / 360;
  const s = hsl.s / 100;
  const l = hsl.l / 100;

  if (s === 0) {
    const v = Math.round(l * 255);
    return `#${v.toString(16).padStart(2, "0").repeat(3)}`;
  }

  const hue2rgb = (p: number, q: number, t: number) => {
    const tt = t < 0 ? t + 1 : t > 1 ? t - 1 : t;
    if (tt < 1 / 6) return p + (q - p) * 6 * tt;
    if (tt < 1 / 2) return q;
    if (tt < 2 / 3) return p + (q - p) * (2 / 3 - tt) * 6;
    return p;
  };

  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const r = Math.round(hue2rgb(p, q, h + 1 / 3) * 255);
  const g = Math.round(hue2rgb(p, q, h) * 255);
  const b = Math.round(hue2rgb(p, q, h - 1 / 3) * 255);

  return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${b.toString(16).padStart(2, "0")}`;
}

function relativeLuminance(hex: string): number {
  const raw = hex.replace("#", "");
  const toLinear = (c: number) => {
    const srgb = c / 255;
    return srgb <= 0.03928 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
  };
  const r = toLinear(parseInt(raw.substring(0, 2), 16));
  const g = toLinear(parseInt(raw.substring(2, 4), 16));
  const b = toLinear(parseInt(raw.substring(4, 6), 16));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/**
 * Choose a text colour (light or dark) that has the best perceived
 * contrast against the given background hex, using the APCA model.
 */
export function textColorForBg(bgHex: string): string {
  return textColorForGradient([bgHex]);
}

/**
 * Given user-selected colours, generate harmonious companion colours
 * to fill a visually pleasing palette. Uses analogous hue shifts and
 * lightness / saturation variations derived from the input set.
 */
export function generateHarmoniousColors(userColors: string[]): string[] {
  if (userColors.length === 0) return [];
  const hslColors = userColors.map(hexToHsl);

  const avgH = averageHue(hslColors);
  const avgS = Math.round(hslColors.reduce((sum, c) => sum + c.s, 0) / hslColors.length);
  const avgL = Math.round(hslColors.reduce((sum, c) => sum + c.l, 0) / hslColors.length);

  const companions: string[] = [];

  companions.push(hslToHex({ h: (avgH + 30) % 360, s: clamp(avgS - 10, 15, 90), l: clamp(avgL - 8, 10, 85) }));
  companions.push(hslToHex({ h: (avgH + 330) % 360, s: clamp(avgS - 5, 15, 90), l: clamp(avgL + 8, 10, 85) }));
  companions.push(hslToHex({ h: (avgH + 15) % 360, s: clamp(avgS + 10, 15, 90), l: clamp(avgL - 15, 10, 85) }));

  return companions;
}

/**
 * Compute a border colour that complements the gradient.
 * Picks a slightly lighter, more saturated variation of the average hue.
 */
export function borderColorFromPalette(userColors: string[]): string {
  if (userColors.length === 0) return "rgba(255,255,255,0.12)";
  const hslColors = userColors.map(hexToHsl);
  const avgH = averageHue(hslColors);
  const avgS = Math.round(hslColors.reduce((sum, c) => sum + c.s, 0) / hslColors.length);
  const avgL = Math.round(hslColors.reduce((sum, c) => sum + c.l, 0) / hslColors.length);
  return hslToHex({ h: avgH, s: clamp(avgS + 15, 20, 80), l: clamp(avgL + 20, 30, 70) });
}

/**
 * Compute APCA (Advanced Perceptual Contrast Algorithm) lightness
 * contrast between a background and text colour.
 *
 * Returns a signed value: positive = dark text on light bg,
 * negative = light text on dark bg. Higher absolute value = better
 * perceived readability. APCA properly accounts for the polarity
 * asymmetry where light text on dark/saturated backgrounds is more
 * legible than the reverse at equal luminance differences.
 */
function apcaLightness(bgY: number, txtY: number): number {
  const bgC = bgY > 0.022 ? bgY : bgY + (0.022 - bgY) ** 1.414;
  const txtC = txtY > 0.022 ? txtY : txtY + (0.022 - txtY) ** 1.414;

  if (bgC > txtC) {
    const sapc = (bgC ** 0.56 - txtC ** 0.57) * 1.14;
    return sapc < 0.1 ? 0 : (sapc - 0.027) * 100;
  }
  const sapc = (bgC ** 0.65 - txtC ** 0.62) * 1.14;
  return sapc > -0.1 ? 0 : (sapc + 0.027) * 100;
}

/**
 * Find the best text colour for content placed over a gradient.
 *
 * Uses APCA (the contrast model for WCAG 3.0) which correctly handles
 * polarity: light text on saturated/dark backgrounds is perceived as
 * more readable than dark text at the same mathematical luminance
 * difference. This fixes issues with pure red, blue, and other
 * saturated mid-tone colours where WCAG 2.x picks dark text incorrectly.
 *
 * Evaluates both white and black text against every gradient stop,
 * takes the worst-case (minimum |Lc|) for each, and picks the winner.
 */
export function textColorForGradient(userColors: string[]): string {
  if (userColors.length === 0) return "#ffffff";

  const WHITE_Y = 1.0;
  const BLACK_Y = 0.012;

  let worstWhiteLc = Infinity;
  let worstBlackLc = Infinity;

  for (const c of userColors) {
    const bgY = relativeLuminance(c);
    const lcWhite = Math.abs(apcaLightness(bgY, WHITE_Y));
    const lcBlack = Math.abs(apcaLightness(bgY, BLACK_Y));
    if (lcWhite < worstWhiteLc) worstWhiteLc = lcWhite;
    if (lcBlack < worstBlackLc) worstBlackLc = lcBlack;
  }

  return worstWhiteLc >= worstBlackLc ? "#ffffff" : "#111111";
}

export function hexToRgba(hex: string, alpha: number): string {
  const raw = hex.replace("#", "");
  const r = parseInt(raw.substring(0, 2), 16);
  const g = parseInt(raw.substring(2, 4), 16);
  const b = parseInt(raw.substring(4, 6), 16);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/** Max colours used in the background gradient (the rest become accents). */
const MAX_GRADIENT_STOPS = 3;

/**
 * Build a CSS gradient with optional glass-level transparency.
 * Only the first 3 user colours are used as gradient stops; extras should
 * be consumed as accent / border colours via `resolveThemePalette`.
 */
export function buildGradient(userColors: string[], angle = 135, alpha = 1): string {
  if (userColors.length === 0) return "var(--color-glass)";
  const stops = userColors.slice(0, MAX_GRADIENT_STOPS);
  const toStop = (c: string) => (alpha < 1 ? hexToRgba(c, alpha) : c);
  if (stops.length === 1) {
    const hsl = hexToHsl(stops[0]);
    const companion = hslToHex({
      h: hsl.h,
      s: clamp(hsl.s - 5, 10, 90),
      l: clamp(hsl.l + (hsl.l > 50 ? -12 : 12), 10, 85),
    });
    return `linear-gradient(${angle}deg, ${toStop(stops[0])}, ${toStop(companion)})`;
  }
  if (stops.length === 2) {
    return `linear-gradient(${angle}deg, ${stops.map(toStop).join(", ")})`;
  }
  return `linear-gradient(${angle}deg, ${stops.map(toStop).join(", ")})`;
}

export interface ThemePalette {
  gradient: string;
  borderColor: string;
  accentColor?: string;
  textColor: string;
}

/**
 * Derive a full theme palette from the user's colour picks.
 *
 * - Colours 1-3 form the background gradient.
 * - Colour 4 becomes the border accent (falls back to computed).
 * - Colour 5 becomes a general accent (status highlights, etc.).
 * - Text colour is always contrast-aware against the gradient colours.
 */
export function resolveThemePalette(
  userColors: string[],
  glass = false,
): ThemePalette {
  const alpha = glass ? 0.55 : 1;
  const gradientColors = userColors.slice(0, MAX_GRADIENT_STOPS);
  const extras = userColors.slice(MAX_GRADIENT_STOPS);

  return {
    gradient: buildGradient(userColors, 135, alpha),
    borderColor: extras[0] ?? borderColorFromPalette(gradientColors),
    accentColor: extras[1],
    textColor: textColorForGradient(gradientColors),
  };
}

/**
 * Generate a random set of 1-5 visually cohesive theme colours.
 *
 * Uses an analogous palette: all hues stay within a ~60 degree arc,
 * with gentle saturation and lightness variation so the result looks
 * harmonious rather than like a rainbow.
 */
export function randomThemeColors(): string[] {
  const count = 1 + Math.floor(Math.random() * 5);
  const baseHue = Math.floor(Math.random() * 360);
  const baseSat = 35 + Math.floor(Math.random() * 35);
  const baseLit = 25 + Math.floor(Math.random() * 25);
  const colors: string[] = [];
  for (let i = 0; i < count; i++) {
    const hueShift = (i / Math.max(count - 1, 1)) * 60 - 30;
    const hue = (baseHue + hueShift + Math.random() * 10 - 5 + 360) % 360;
    const sat = clamp(baseSat + (Math.random() * 20 - 10), 20, 80);
    const lit = clamp(baseLit + i * 6 + (Math.random() * 8 - 4), 15, 55);
    colors.push(hslToHex({ h: Math.round(hue), s: Math.round(sat), l: Math.round(lit) }));
  }
  return colors;
}

function averageHue(colors: HSL[]): number {
  let sinSum = 0;
  let cosSum = 0;
  for (const c of colors) {
    const rad = (c.h * Math.PI) / 180;
    sinSum += Math.sin(rad);
    cosSum += Math.cos(rad);
  }
  let avg = (Math.atan2(sinSum, cosSum) * 180) / Math.PI;
  if (avg < 0) avg += 360;
  return Math.round(avg);
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
