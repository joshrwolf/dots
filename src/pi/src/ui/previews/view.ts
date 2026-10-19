import { readableForeground } from "../color.ts";
import { displayLimits } from "../policy.ts";
import type { Theme } from "@earendil-works/pi-coding-agent";
import { mixColors, parseColor, truncateToWidth, visibleWidth, wrapTextWithAnsi, type Color, type Component } from "@earendil-works/pi-tui";
import { type SyntaxToken, type Tokens, type SyntaxService } from "./syntax.ts";
import { sourceText as clean } from "../text.ts";
import { pairLines, wordChanges, type Line, type Pair, type Patch, type Range } from "./patch.ts";

const MAX_VISUAL_LINES = displayLimits.previewRows;
const graphemes = new Intl.Segmenter(undefined, { granularity: "grapheme" });

function limitLines(lines: string[], width: number): string[] {
  if (lines.length < MAX_VISUAL_LINES) return lines;
  return [...lines.slice(0, MAX_VISUAL_LINES - 1), truncateToWidth("… display limited; inspect the recorded tool result for the remainder", width)];
}

function color(line: Line): "toolDiffAdded" | "toolDiffRemoved" | "toolDiffContext" {
  return line.kind === "added" ? "toolDiffAdded" : line.kind === "removed" ? "toolDiffRemoved" : "toolDiffContext";
}

function chunks(text: string, width: number): string[] {
  const room = Math.max(1, width);
  const budget = room * MAX_VISUAL_LINES;
  const limited = visibleWidth(text) > budget;
  const prepared = limited ? truncateToWidth(text, budget, "") : text;
  const lines = wrapTextWithAnsi(prepared, room).map((part) => truncateToWidth(part, room, ""));
  if (limited) lines.push(truncateToWidth("… display limited", room));
  return lines;
}

function background(theme: Theme, line: Line, strong = false): Color | undefined {
  if (line.kind !== "added" && line.kind !== "removed") return undefined;
  const base = line.kind === "removed" ? theme.colors.toolErrorBg : theme.colors.toolSuccessBg;
  return mixColors(base, theme.colors[color(line)], strong ? 0.28 : 0.09, "srgb");
}

// Compose Shiki token colors and changed-word backgrounds before wrapping. Walk
// graphemes so a highlight boundary never cuts an emoji or combining sequence.
function paint(text: string, tokens: readonly SyntaxToken[] | undefined, theme: Theme, line?: Line, ranges: Range[] = []): string {
  const bg = line ? background(theme, line) : undefined;
  const strong = line ? background(theme, line, true) : undefined;
  if (text.length > displayLimits.syntaxLineCodeUnits || (!tokens && ranges.length === 0)) return theme.style(text, { fg: bg ? readableForeground(theme.colors.text, bg) : theme.colors.text, ...(bg ? { bg } : {}) });
  const parts = tokens ?? [{ content: text, offset: 0 }];
  const ends: number[] = [];
  let total = 0;
  for (const token of parts) ends.push(total += token.content.length);
  let tokenIndex = 0;
  let rangeIndex = 0;
  let run = "";
  let previous = "";
  let output = "";
  let currentStyle: Parameters<Theme["style"]>[1] = {};
  const flush = () => { if (run) output += theme.style(run, currentStyle); run = ""; };
  for (const { segment, index } of graphemes.segment(text)) {
    while (tokenIndex < parts.length - 1 && index >= ends[tokenIndex]!) tokenIndex++;
    while (rangeIndex < ranges.length && ranges[rangeIndex]!.end <= index) rangeIndex++;
    const changed = ranges[rangeIndex] !== undefined && ranges[rangeIndex]!.start < index + segment.length;
    const token = parts[tokenIndex];
    const key = `${token?.color ?? ""}/${token?.fontStyle ?? 0}/${changed}`;
    if (key !== previous) {
      flush();
      const font = token?.fontStyle ?? 0;
      const foreground = token?.color ? parseColor(token.color) : theme.colors.text;
      const background = changed ? strong : bg;
      currentStyle = { fg: background ? readableForeground(foreground, background) : foreground, ...(background ? { bg: background } : {}), italic: !!(font & 1), bold: !!(font & 2), underline: !!(font & 4) };
      previous = key;
    }
    run += segment;
  }
  flush();
  return output;
}

function fill(text: string, room: number, theme: Theme, bg?: Color): string {
  return text + theme.style(" ".repeat(Math.max(0, room - visibleWidth(text))), { ...(bg ? { bg } : {}) });
}

export interface DiffViewOptions {
  /** Content-sized split columns by default; fill retains equal full-width columns. */
  readonly widthPolicy?: "content" | "fill";
  readonly patch: Patch;
  readonly path: string;
  readonly theme: Theme;
  readonly syntax: SyntaxService;
  readonly invalidate?: () => void;
}
export interface SourceViewOptions {
  readonly source: string;
  readonly path: string;
  readonly theme: Theme;
  readonly syntax: SyntaxService;
  readonly invalidate?: () => void;
  readonly startLine?: number;
  /** A selected excerpt can end with a real blank row, not an EOF newline. */
  readonly preserveTrailingEmptyLine?: boolean;
}

export class DiffView implements Component {
  private cache: { width: number; lines: string[] } | undefined;
  private colored: Map<Line, string> | undefined;
  private tokens: Map<Line, readonly SyntaxToken[]> | undefined;
  private tokenAppearance: string | undefined;
  private pending: Promise<void> | undefined;
  private readonly patch: Patch;
  private readonly widthPolicy: "content" | "fill";
  private readonly pairs: Pair[][];
  private readonly changes = new Map<Line, Range[]>();
  private readonly path: string;
  private readonly syntax: SyntaxService;
  private generation = 0;
  private theme: Theme;
  private readonly notify: () => void;

  constructor({ patch, path, theme, syntax, widthPolicy = "content", invalidate: notify = () => {} }: DiffViewOptions) {
    this.widthPolicy = widthPolicy;
    let count = 0;
    let size = 0;
    for (const hunk of patch.hunks) for (const line of hunk.lines) {
      count++;
      size += line.text.length;
      if (count > displayLimits.patchLines || size > displayLimits.sourceCodeUnits) throw new RangeError("Diff exceeds preview input limits");
    }
    this.patch = { ...patch, hunks: patch.hunks.map((hunk) => ({ ...hunk, lines: hunk.lines.map((line) => ({ ...line, text: clean(line.text) })) })) };
    this.path = path;
    this.syntax = syntax;
    this.theme = theme;
    this.notify = notify;
    this.pairs = this.patch.hunks.map((hunk) => pairLines(hunk.lines));
    for (const pairs of this.pairs) for (const { left, right } of pairs) {
      if (left?.kind === "removed" && right?.kind === "added") {
        const ranges = wordChanges(left.text, right.text);
        this.changes.set(left, ranges.before);
        this.changes.set(right, ranges.after);
      }
    }
  }

  setTheme(theme: Theme): void {
    if (theme !== this.theme) { this.theme = theme; this.invalidate(); }
  }

  invalidate(): void {
    this.cache = undefined;
    this.colored = undefined;
  }

  // Plain text appears immediately. Async grammar loading invalidates only this
  // component; completed tokens survive resizes and ordinary invalidation.
  prepare(): Promise<void> {
    const appearance = this.theme.appearance;
    if (this.tokenAppearance === appearance && this.pending) return this.pending;
    const generation = ++this.generation;
    this.tokenAppearance = appearance;
    this.tokens = undefined;
    const tokens = new Map<Line, readonly SyntaxToken[]>();
    const size = this.patch.hunks.reduce((n, h) => n + h.lines.reduce((sum, line) => sum + line.text.length, 0), 0);
    this.pending = Promise.all(this.patch.hunks.flatMap((hunk) => ["added", "removed"].map(async (excluded) => {
      const side = hunk.lines.filter((line) => line.kind !== excluded && line.kind !== "note");
      const highlighted = size <= displayLimits.syntaxCodeUnits && this.patch.hunks.length <= displayLimits.syntaxHunks ? await this.syntax.highlight(side.map((line) => line.text).join("\n"), this.path, appearance) : [];
      side.forEach((line, i) => { if (highlighted[i]) tokens.set(line, highlighted[i]!); });
    }))).then(() => {
      if (!this.syntax.active || this.generation !== generation) return;
      this.tokens = tokens;
      this.invalidate();
      this.notify();
    });
    return this.pending;
  }

  render(width: number): string[] {
    if (width <= 0) return [];
    void this.prepare();
    if (this.cache?.width === width) return this.cache.lines;
    this.colored ??= new Map();
    const numberWidth = Math.max(1, ...this.patch.hunks.flatMap((h) => h.lines.map((line) => String(Math.max(line.old ?? 0, line.next ?? 0)).length)));
    const gutter = numberWidth + 3;
    const available = Math.max(0, width - 3);
    let leftWidth = Math.floor(available / 2);
    let rightWidth = available - leftWidth;
    const allLines = this.patch.hunks.flatMap((h) => h.lines).filter((line) => line.kind !== "note");
    const measures = allLines.map((line) => ({ line, size: visibleWidth(line.text) }));
    if (this.widthPolicy === "content") {
      const preferredLeft = measures.reduce((size, { line, size: text }) => line.kind === "added" ? size : Math.max(size, gutter + text), 6);
      const preferredRight = measures.reduce((size, { line, size: text }) => line.kind === "removed" ? size : Math.max(size, gutter + text), 5);
      leftWidth = Math.min(preferredLeft, leftWidth);
      rightWidth = Math.min(preferredRight, available - leftWidth);
      leftWidth = Math.min(preferredLeft, available - rightWidth);
    }
    const wrapping = measures.filter(({ line, size }) => size > (line.kind === "removed" ? leftWidth : line.kind === "added" ? rightWidth : Math.min(leftWidth, rightWidth)) - gutter).length;
    const split = width >= 120 && Math.floor(available / 2) - gutter >= 40 && wrapping <= allLines.length * 0.25;
    const lines: string[] = [];
    const theme = this.theme;
    if (split) lines.push(theme.fg("muted", "before".padEnd(leftWidth) + " │ after"));
    const cell = (line: Line | undefined, side: "old" | "next", room: number): string[] => {
      if (!line) return [theme.fg("dim", "·")];
      const marker = line.kind === "removed" ? "−" : line.kind === "added" ? "+" : " ";
      const number = line[side];
      const prefix = `${marker} ${number === undefined ? "".padStart(numberWidth) : String(number).padStart(numberWidth)} `;
      const bg = background(theme, line);
      if (room <= gutter) return [truncateToWidth(theme.fg(color(line), prefix + line.text), room, "")];
      let painted = this.colored!.get(line);
      if (painted === undefined) {
        painted = paint(line.text, this.tokens?.get(line), theme, line, this.changes.get(line));
        this.colored!.set(line, painted);
      }
      return chunks(painted, room - gutter).map((part, i) => {
        const head = theme.style(i === 0 ? prefix : " ".repeat(gutter), { fg: bg ? readableForeground(theme.colors[color(line)], bg) : theme.colors[color(line)], ...(bg ? { bg } : {}) });
        return fill(head + part, room, theme, bg);
      });
    };
    outer: for (const [index, hunk] of this.patch.hunks.entries()) {
      if (this.patch.hunks.length > 1) lines.push(theme.fg("muted", truncateToWidth(`lines ${hunk.oldStart} → ${hunk.newStart}`, width)));
      if (split) {
        for (const pair of this.pairs[index]!) {
          if (pair.left?.kind === "note") lines.push(theme.fg("muted", truncateToWidth(pair.left.text, width)));
          else {
            const left = cell(pair.left, "old", leftWidth);
            const right = cell(pair.right, "next", rightWidth);
            for (let i = 0; i < Math.max(left.length, right.length); i++) {
              lines.push(fill(left[i] ?? "", leftWidth, theme) + theme.fg("borderMuted", " │ ") + (right[i] ?? ""));
              if (lines.length >= MAX_VISUAL_LINES) break outer;
            }
          }
          if (lines.length >= MAX_VISUAL_LINES) break outer;
        }
      } else {
        for (const line of hunk.lines) {
          if (line.kind === "note") lines.push(theme.fg("muted", truncateToWidth(line.text, width)));
          else lines.push(...cell(line, line.kind === "removed" ? "old" : "next", width));
          if (lines.length >= MAX_VISUAL_LINES) break outer;
        }
      }
    }
    this.cache = { width, lines: limitLines(lines, width) };
    return this.cache.lines;
  }
}

export class SourceView implements Component {
  private readonly startLine: number;
  private readonly preserveTrailingEmptyLine: boolean;
  private readonly source: string;
  private readonly truncated: boolean;
  private readonly path: string;
  private readonly syntax: SyntaxService;
  private generation = 0;
  private theme: Theme;
  private readonly notify: () => void;
  private tokens: Tokens | undefined;
  private appearance: string | undefined;
  private pending: Promise<void> | undefined;
  private colored: string[] | undefined;
  private cache: { width: number; lines: string[] } | undefined;

  constructor({ source, path, theme, syntax, invalidate: notify = () => {}, startLine = 1, preserveTrailingEmptyLine = false }: SourceViewOptions) {
    this.startLine = Number.isSafeInteger(startLine) && startLine > 0 ? startLine : 1;
    const bounded = clean(source.slice(0, displayLimits.sourceCodeUnits));
    let end = bounded.length;
    let cursor = 0;
    for (let line = 0; line < displayLimits.sourceLines; line++) {
      const newline = bounded.indexOf("\n", cursor);
      if (newline < 0) break;
      cursor = newline + 1;
      if (line === displayLimits.sourceLines - 1) end = cursor;
    }
    this.truncated = source.length > displayLimits.sourceCodeUnits || end < bounded.length;
    this.source = bounded.slice(0, end);
    this.preserveTrailingEmptyLine = preserveTrailingEmptyLine && !this.truncated;
    this.path = path;
    this.syntax = syntax;
    this.theme = theme;
    this.notify = notify;
  }
  setTheme(theme: Theme): void {
    if (theme !== this.theme) { this.theme = theme; this.invalidate(); }
  }
  invalidate(): void { this.cache = undefined; this.colored = undefined; }
  prepare(): Promise<void> {
    const appearance = this.theme.appearance;
    if (this.appearance === appearance && this.pending) return this.pending;
    const generation = ++this.generation;
    this.appearance = appearance;
    this.tokens = undefined;
    this.pending = this.syntax.highlight(this.source, this.path, appearance).then((tokens) => {
      if (!this.syntax.active || this.generation !== generation) return;
      this.tokens = tokens;
      this.invalidate();
      this.notify();
    });
    return this.pending;
  }
  render(width: number): string[] {
    if (width <= 0) return [];
    void this.prepare();
    if (this.cache?.width === width) return this.cache.lines;
    const source = this.source.split("\n");
    if (!this.preserveTrailingEmptyLine && source.at(-1) === "") source.pop();
    this.colored ??= source.slice(0, MAX_VISUAL_LINES).map((line, i) => paint(line, this.tokens?.[i], this.theme));
    const gutter = String(this.startLine + Math.max(0, source.length - 1)).length + 2;
    const lines: string[] = [];
    for (const [i, code] of this.colored.entries()) {
      const head = this.theme.fg("muted", `${String(this.startLine + i).padStart(gutter - 2)}  `);
      for (const [j, part] of chunks(code, Math.max(1, width - gutter)).entries()) {
        lines.push(truncateToWidth((j === 0 ? head : " ".repeat(gutter)) + part, width, ""));
        if (lines.length >= MAX_VISUAL_LINES) break;
      }
      if (lines.length >= MAX_VISUAL_LINES) break;
    }
    if (this.truncated && lines.length < MAX_VISUAL_LINES) lines.push(truncateToWidth("… source exceeds display limit", width));
    this.cache = { width, lines: limitLines(lines, width) };
    return this.cache.lines;
  }
}
