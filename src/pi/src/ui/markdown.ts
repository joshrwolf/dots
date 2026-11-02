import {
  getMarkdownTheme,
  type KeybindingHints,
  type Theme,
} from '@earendil-works/pi-coding-agent';
import {
  Markdown,
  truncateToWidth,
  type Component,
} from '@earendil-works/pi-tui';
import {sourceText, plainLines, displayLimits} from '@joshrwolf/pi-kit/ui';
import {expandCue} from './card.ts';

/** Human prose only; recorded tool output remains plain text unless its owner
 * knows it is Markdown. Bounds apply to preparation as well as rendered rows.
 * Pass the host's hints on a collapsed, expandable row to cue expansion;
 * elsewhere a limit points to the original document. */
export class MarkdownView implements Component {
  private readonly markdown: Markdown;
  private readonly limited: boolean;
  private readonly rows: number;
  private readonly theme: Theme;
  private readonly limitNote: string;
  constructor(
    text: string,
    theme: Theme,
    rows: number = displayLimits.outputRows,
    expand?: Pick<KeybindingHints, 'keyText'>,
  ) {
    const bounded = sourceText(text.slice(0, 24_576));
    const lines = bounded.split('\n');
    this.limited = text.length > 24_576 || lines.length > 400;
    this.markdown = new Markdown(
      lines.slice(0, 400).join('\n'),
      0,
      0,
      getMarkdownTheme(),
    );
    this.rows = rows;
    this.theme = theme;
    this.limitNote = expand
      ? `… ${expandCue(expand)}`.trimEnd()
      : '… display limited; inspect the original document';
  }
  invalidate(): void {
    this.markdown.invalidate();
  }
  render(width: number): string[] {
    if (width <= 0) return [];
    const lines = this.markdown
      .render(width)
      .map(line => truncateToWidth(line, width, ''));
    const limited = this.limited || lines.length > this.rows;
    return [
      ...lines.slice(0, this.rows),
      ...(limited
        ? plainLines(this.limitNote, width, 1).map(line =>
            this.theme.fg('dim', line),
          )
        : []),
    ];
  }
}
