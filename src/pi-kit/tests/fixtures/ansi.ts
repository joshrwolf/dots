import {visibleWidth} from '@earendil-works/pi-tui';
// Exact-version host oracle, separate from the production surface compositor.
import {
  extractAnsiCode,
  getActiveBackgroundAnsi,
} from '../../../../node_modules/@earendil-works/pi-tui/dist/utils.js';

/** Effective background at each occupied terminal column, using Pi's ANSI
 * tracker and native grapheme widths rather than the compositor's logic. */
export function backgroundCells(line: string): string[] {
  const cells: string[] = [];
  const segmenter = new Intl.Segmenter(undefined, {granularity: 'grapheme'});
  let at = 0;
  while (at < line.length) {
    const control = extractAnsiCode(line, at);
    if (control) {
      at += control.length;
      continue;
    }
    const escape = line.indexOf('\x1b', at);
    const end = escape < 0 ? line.length : Math.max(at + 1, escape);
    const background = getActiveBackgroundAnsi(line.slice(0, at));
    for (const {segment} of segmenter.segment(line.slice(at, end)))
      cells.push(...Array<string>(visibleWidth(segment)).fill(background));
    at = end;
  }
  return cells;
}
