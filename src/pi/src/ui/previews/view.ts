import {displayLimits, sourceText as clean} from '@joshrwolf/pi-kit/ui';
import {
  getLanguageFromPath,
  highlightCode,
  renderDiff,
  type Theme,
} from '@earendil-works/pi-coding-agent';
import {
  truncateToWidth,
  visibleWidth,
  wrapTextWithAnsi,
  type Component,
} from '@earendil-works/pi-tui';

const MAX_VISUAL_LINES = displayLimits.previewRows;

function limitLines(lines: string[], width: number): string[] {
  if (lines.length < MAX_VISUAL_LINES) return lines;
  return [
    ...lines.slice(0, MAX_VISUAL_LINES - 1),
    truncateToWidth(
      '… display limited; inspect the recorded tool result for the remainder',
      width,
    ),
  ];
}

function chunks(text: string, width: number): string[] {
  const room = Math.max(1, width);
  const budget = room * MAX_VISUAL_LINES;
  const limited = visibleWidth(text) > budget;
  const prepared = limited ? truncateToWidth(text, budget, '') : text;
  const lines = wrapTextWithAnsi(prepared, room).map(part =>
    truncateToWidth(part, room, ''),
  );
  if (limited) lines.push(truncateToWidth('… display limited', room));
  return lines;
}

/** Bound by code units and lines before any styling work. */
function bound(text: string): {text: string; truncated: boolean} {
  const bounded = clean(text.slice(0, displayLimits.sourceCodeUnits));
  let end = bounded.length;
  let cursor = 0;
  for (let line = 0; line < displayLimits.sourceLines; line++) {
    const newline = bounded.indexOf('\n', cursor);
    if (newline < 0) break;
    cursor = newline + 1;
    if (line === displayLimits.sourceLines - 1) end = cursor;
  }
  return {
    text: bounded.slice(0, end),
    truncated:
      text.length > displayLimits.sourceCodeUnits || end < bounded.length,
  };
}

/** Pi's highlighter for a whole document; oversized input stays plain. */
function highlight(source: string, path: string, theme: Theme): string[] {
  const language = getLanguageFromPath(path);
  if (
    language &&
    source.length <= displayLimits.syntaxCodeUnits &&
    !source
      .split('\n')
      .some(line => line.length > displayLimits.syntaxLineCodeUnits)
  )
    return highlightCode(source, language);
  return source.split('\n').map(line => theme.fg('toolOutput', line));
}

/** The recorded edit diff in Pi's own diff styling. */
export class DiffView implements Component {
  private readonly diff: string;
  private readonly truncated: boolean;
  private theme: Theme;
  private styled: string[] | undefined;
  private cache: {width: number; lines: string[]} | undefined;

  constructor(diff: string, theme: Theme) {
    const bounded = bound(diff);
    this.diff = bounded.text.replace(/\n$/, '');
    this.truncated = bounded.truncated;
    this.theme = theme;
  }
  setTheme(theme: Theme): void {
    if (theme !== this.theme) {
      this.theme = theme;
      this.invalidate();
    }
  }
  invalidate(): void {
    this.cache = undefined;
    this.styled = undefined;
  }
  render(width: number): string[] {
    if (width <= 0) return [];
    if (this.cache?.width === width) return this.cache.lines;
    this.styled ??= renderDiff(this.diff).split('\n');
    const lines: string[] = [];
    for (const line of this.styled) {
      lines.push(...chunks(line, width));
      if (lines.length >= MAX_VISUAL_LINES) break;
    }
    if (this.truncated && lines.length < MAX_VISUAL_LINES)
      lines.push(truncateToWidth('… diff exceeds display limit', width));
    this.cache = {width, lines: limitLines(lines, width)};
    return this.cache.lines;
  }
}

export interface SourceViewOptions {
  readonly source: string;
  readonly path: string;
  readonly theme: Theme;
  readonly startLine?: number;
  /** A selected excerpt can end with a real blank row, not an EOF newline. */
  readonly preserveTrailingEmptyLine?: boolean;
  /** False shows plain text, e.g. while source still streams. */
  readonly highlight?: boolean;
}

export class SourceView implements Component {
  private readonly startLine: number;
  private readonly preserveTrailingEmptyLine: boolean;
  private readonly highlight: boolean;
  private readonly source: string;
  private readonly truncated: boolean;
  private readonly path: string;
  private theme: Theme;
  private colored: string[] | undefined;
  private cache: {width: number; lines: string[]} | undefined;

  constructor({
    source,
    path,
    theme,
    startLine = 1,
    preserveTrailingEmptyLine = false,
    highlight = true,
  }: SourceViewOptions) {
    this.highlight = highlight;
    this.startLine =
      Number.isSafeInteger(startLine) && startLine > 0 ? startLine : 1;
    const bounded = bound(source);
    this.truncated = bounded.truncated;
    this.source = bounded.text;
    this.preserveTrailingEmptyLine =
      preserveTrailingEmptyLine && !this.truncated;
    this.path = path;
    this.theme = theme;
  }
  setTheme(theme: Theme): void {
    if (theme !== this.theme) {
      this.theme = theme;
      this.invalidate();
    }
  }
  invalidate(): void {
    this.cache = undefined;
    this.colored = undefined;
  }
  render(width: number): string[] {
    if (width <= 0) return [];
    if (this.cache?.width === width) return this.cache.lines;
    const source = this.source.split('\n');
    if (!this.preserveTrailingEmptyLine && source.at(-1) === '') source.pop();
    const shown = source.slice(0, MAX_VISUAL_LINES);
    this.colored ??= this.highlight
      ? highlight(shown.join('\n'), this.path, this.theme)
      : shown.map(line => this.theme.fg('toolOutput', line));
    const gutter =
      String(this.startLine + Math.max(0, source.length - 1)).length + 2;
    const lines: string[] = [];
    for (const [i, code] of this.colored.entries()) {
      const head = this.theme.fg(
        'muted',
        `${String(this.startLine + i).padStart(gutter - 2)}  `,
      );
      for (const [j, part] of chunks(
        code,
        Math.max(1, width - gutter),
      ).entries()) {
        lines.push(
          truncateToWidth(
            (j === 0 ? head : ' '.repeat(gutter)) + part,
            width,
            '',
          ),
        );
        if (lines.length >= MAX_VISUAL_LINES) break;
      }
      if (lines.length >= MAX_VISUAL_LINES) break;
    }
    if (this.truncated && lines.length < MAX_VISUAL_LINES)
      lines.push(truncateToWidth('… source exceeds display limit', width));
    this.cache = {width, lines: limitLines(lines, width)};
    return this.cache.lines;
  }
}
