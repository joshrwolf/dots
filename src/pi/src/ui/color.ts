import { colorToHex, colorToRgb, mixColors, rgbColor, type Color } from "@earendil-works/pi-tui";

const black = rgbColor(0, 0, 0);
const white = rgbColor(255, 255, 255);
// Concrete color pairs, not theme identities or session/source data. Bounded
// across all rows; changing themes naturally produces different keys.
const adjusted = new Map<string, Color>();

function luminance(color: Color): number {
  const { r, g, b } = colorToRgb(color);
  const linear = (value: number) => { const s = value / 255; return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
}
function contrast(a: Color, b: Color): number {
  const first = luminance(a), second = luminance(b);
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
}

/** Preserve syntax hues when readable; otherwise mix only as far as needed.
 * Targets 4.5:1 in concrete sRGB; terminal palette quantization can differ.
 */
export function readableForeground(foreground: Color, background: Color): Color {
  const key = `${colorToHex(foreground)}/${colorToHex(background)}`;
  const cached = adjusted.get(key);
  if (cached) return cached;
  let result = foreground;
  if (contrast(foreground, background) < 4.5) {
    const anchor = contrast(black, background) >= contrast(white, background) ? black : white;
    let low = 0, high = 1;
    for (let step = 0; step < 12; step++) {
      const middle = (low + high) / 2;
      if (contrast(mixColors(foreground, anchor, middle, "srgb"), background) >= 4.5) high = middle;
      else low = middle;
    }
    result = mixColors(foreground, anchor, high, "srgb");
  }
  if (adjusted.size >= 512) adjusted.clear();
  adjusted.set(key, result);
  return result;
}
