import {truncateToWidth, visibleWidth} from '@earendil-works/pi-tui';

/** Precomputed SGR colors from the host theme or its color helpers. Terminal
 * default colors remain SGR 39/49 rather than guessed RGB values. */
export interface SurfaceColors {
  readonly fg: string;
  readonly bg: string;
}

// Match whole control strings before CSI, so SGR-looking bytes in a hyperlink,
// image or other opaque payload are never rewritten. Unterminated strings own
// the remainder of the line. Layout and terminal-width handling stay with Pi.
const controls =
  /\x1b(?:\][\s\S]*?(?:\x07|\x1b\\|$)|[P_X^][\s\S]*?(?:\x1b\\|$)|\[[0-?]*[ -/]*[@-~])/g;
const reset = '\x1b[0m\x1b]8;;\x07';

/** Paint a bounded surface whose defaults survive child style resets.
 * Explicit child colors and attributes win; padding belongs to the surface.
 * Each line closes its styles and hyperlinks, allowing surfaces to nest. */
export function paintSurfaceLine(
  line: string,
  width: number,
  {fg, bg}: SurfaceColors,
): string {
  if (width <= 0) return '';
  const bounded = truncateToWidth(line, width, '');
  const content = bounded.replace(controls, sequence => {
    if (!/^\x1b\[[\d;:]*m$/.test(sequence)) return sequence;
    const codes = sequence.slice(2, -1).split(';');
    let foreground = false;
    let background = false;
    for (let i = 0; i < codes.length; i++) {
      const parameter = codes[i]!;
      const code = Number(parameter.split(':')[0]);
      if (code === 0) {
        foreground = true;
        background = true;
      } else if (code === 39) foreground = true;
      else if (code === 49) background = true;
      else if (
        code === 38 ||
        (code >= 30 && code <= 37) ||
        (code >= 90 && code <= 97)
      )
        foreground = false;
      else if (
        code === 48 ||
        (code >= 40 && code <= 47) ||
        (code >= 100 && code <= 107)
      )
        background = false;
      // Extended-color payloads are atomic: RGB channels and palette indexes
      // can equal reset codes. Colon-form payloads already occupy one field.
      if (
        (code === 38 || code === 48 || code === 58) &&
        !parameter.includes(':')
      ) {
        if (codes[i + 1] === '2') i += 4;
        else if (codes[i + 1] === '5') i += 2;
      }
    }
    return sequence + (foreground ? fg : '') + (background ? bg : '');
  });
  return (
    reset +
    fg +
    bg +
    content +
    reset +
    fg +
    bg +
    ' '.repeat(Math.max(0, width - visibleWidth(bounded))) +
    reset
  );
}
