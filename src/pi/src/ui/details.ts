import type {Theme} from '@earendil-works/pi-coding-agent';
import {truncateToWidth, type Component} from '@earendil-works/pi-tui';
import {plainLines} from '@joshrwolf/pi-kit/ui';

type Caption = string | ((width: number) => string);

/** Shared expanded-detail treatment; children still own content and layout. */
export class DetailView implements Component {
  private child: Component;
  private caption: Caption;
  private theme: Theme;
  // Framing measures every row, so keep the last frame's rows by source line.
  // Rows that persist across frames or updates (cached previews, unchanged
  // output) are not measured again; only the current frame's rows are retained.
  private cache:
    | {
        content: string[];
        caption: string;
        width: number;
        lines: string[];
        framed: Map<string, string>;
      }
    | undefined;
  constructor(child: Component, caption: Caption, theme: Theme) {
    this.child = child;
    this.caption = caption;
    this.theme = theme;
  }
  /** Replace the framed content; rows already framed at this width are reused. */
  update(child: Component, caption: Caption, theme: Theme): void {
    this.child = child;
    this.caption = caption;
    if (theme !== this.theme) this.cache = undefined;
    this.theme = theme;
  }
  invalidate(): void {
    this.cache = undefined;
    this.child.invalidate();
  }
  render(width: number): string[] {
    if (width <= 0) return [];
    const inset = width >= 8 ? 2 : 0;
    const room = width - inset;
    const caption =
      typeof this.caption === 'function' ? this.caption(width) : this.caption;
    const content = this.child.render(room);
    const cached = this.cache?.width === width ? this.cache : undefined;
    if (cached?.content === content && cached.caption === caption)
      return cached.lines;
    const title = caption
      ? plainLines(caption, room, 8).map(line => this.theme.fg('dim', line))
      : [];
    const rail = inset ? this.theme.fg('borderMuted', '│ ') : '';
    const framed = new Map<string, string>();
    const lines = [
      ...title,
      ...(content.length ? content : [this.theme.fg('dim', '(no content)')]),
    ].map(line => {
      let row = framed.get(line) ?? cached?.framed.get(line);
      if (row === undefined) row = rail + truncateToWidth(line, room);
      framed.set(line, row);
      return row;
    });
    this.cache = {content, caption, width, lines, framed};
    return lines;
  }
}
