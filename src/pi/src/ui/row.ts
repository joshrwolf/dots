import type { Theme } from "@earendil-works/pi-coding-agent";
import { HStack, Text, sliceByColumn, truncateToWidth, visibleWidth, type Component } from "@earendil-works/pi-tui";
import { readableForeground } from "./color.ts";
import { labelText } from "./text.ts";

export type Tone = "accent" | "text" | "toolTitle" | "muted" | "dim" | "success" | "warning" | "error" | "toolDiffAdded" | "toolDiffRemoved";
export interface Segment {
  readonly text: string;
  readonly tone: Tone;
  readonly bold?: boolean;
  /** A compact, padded exceptional-state label; text remains meaningful without color. */
  readonly badge?: "error" | "warning";
  /** Larger values survive longer. Input order always determines display order. */
  readonly priority: number;
  readonly shrink: "clip" | "omit";
  readonly clip?: "end" | "middle";
  /** Desired clip floor (default 0); pass two lifts floors in priority order.
   * `omit` always removes the whole segment rather than preserving a floor.
   */
  readonly minWidth?: number;
  /** Soft right-padding target; spare padding yields before any content shrinks. */
  readonly padTo?: number;
  /** Preferred cell width: clips and pads, then yields under priority/minWidth.
   * Overrides padTo; unlike soft padding, spare cell space is not removed first.
   */
  readonly width?: number;
  /** Override the separator before this segment, only when a prior segment survives in its group. */
  readonly separatorBefore?: string;
}
export type RowLayout = "inline" | "edges";
export interface RowModel {
  /** Inline keeps groups adjacent; edges (default) spends spare width between them.
   * Both layouts use the same group separator and shrink policy.
   */
  readonly layout?: RowLayout;
  /** Leading layout cells, clamped to leave at least one content cell in narrow panes. */
  readonly indent?: number;
  readonly start: readonly Segment[];
  readonly end: readonly Segment[];
  /** Separators belong to the layout, never to optional segment text. */
  readonly separator?: string;
  readonly endSeparator?: string;
  /** Between nonempty groups; defaults to one space in either layout. */
  readonly groupSeparator?: string;
}

export function fitLabel(text: string, width: number, mode: Segment["clip"] = "end"): string {
  if (width <= 0) return "";
  const size = visibleWidth(text);
  if (mode !== "middle" || size <= width || width < 3) return truncateToWidth(text, width);
  const tail = sliceByColumn(text, size - Math.ceil((width - 1) * 0.7), Math.ceil((width - 1) * 0.7), true);
  const head = sliceByColumn(text, 0, width - 1 - visibleWidth(tail), true);
  return truncateToWidth(`${head}…${tail}`, width);
}

/** Information selection is ours; terminal widths and final layout are Pi's. */
export class PriorityRow implements Component {
  private model: RowModel = { start: [], end: [] };
  private theme: Theme | undefined;
  update(model: RowModel, theme: Theme): void { this.model = model; this.theme = theme; }
  invalidate(): void {}

  render(width: number): string[] {
    const theme = this.theme;
    if (!theme || width <= 0) return [];
    const requestedIndent = this.model.indent;
    const indent = requestedIndent !== undefined && Number.isFinite(requestedIndent)
      ? Math.min(width - 1, Math.max(0, Math.floor(requestedIndent))) : 0;
    width -= indent;
    const separator = this.model.separator ?? " ";
    const endSeparator = this.model.endSeparator ?? separator;
    const groupSeparator = this.model.groupSeparator ?? " ";
    const groupGap = visibleWidth(groupSeparator);
    const before = (segment: Segment, side: string) => segment.separatorBefore ?? (side === "end" ? endSeparator : separator);
    const entries = [...this.model.start.map((segment) => ({ segment, side: "start" })), ...this.model.end.map((segment) => ({ segment, side: "end" }))]
      .map((entry) => {
        const clean = labelText(entry.segment.text);
        const text = clean && entry.segment.badge ? ` ${clean} ` : clean;
        const natural = visibleWidth(text);
        const paddingTarget = entry.segment.padTo;
        const fixed = entry.segment.width !== undefined && Number.isFinite(entry.segment.width);
        const size = fixed
          ? Math.min(width, Math.max(0, Math.floor(entry.segment.width!)))
          : Math.max(natural, paddingTarget !== undefined && Number.isFinite(paddingTarget) ? Math.min(width, Math.max(0, Math.floor(paddingTarget))) : 0);
        return { ...entry, text, natural, size, softPadding: fixed ? 0 : size - natural };
      })
      .filter((entry) => entry.natural > 0);
    const total = () => {
      const start = entries.filter((entry) => entry.side === "start" && entry.size > 0).length;
      const end = entries.filter((entry) => entry.side === "end" && entry.size > 0).length;
      const separators = ["start", "end"].reduce((sum, side) => sum + entries.filter((entry) => entry.side === side && entry.size > 0).slice(1)
        .reduce((sum, entry) => sum + visibleWidth(before(entry.segment, side)), 0), 0);
      return entries.reduce((sum, entry) => sum + entry.size, 0) + separators + (start && end ? groupGap : 0);
    };
    const ordered = [...entries].sort((a, b) => a.segment.priority - b.segment.priority);
    // Padding is a preference, not content or a protected minimum.
    for (const entry of ordered) entry.size -= Math.min(entry.softPadding, Math.max(0, total() - width));
    for (const protectMinimum of [true, false]) {
      for (const entry of ordered) {
        const excess = total() - width;
        if (excess <= 0) break;
        const minimum = protectMinimum ? Math.min(entry.size, Math.max(0, entry.segment.minWidth ?? 0)) : 0;
        entry.size = entry.segment.shrink === "omit" ? 0 : Math.max(minimum, entry.size - excess);
      }
    }
    const paint = (segment: Segment, text: string): string => {
      if (segment.badge) {
        const bg = theme.colors[segment.badge === "error" ? "toolErrorBg" : "toolPendingBg"];
        return theme.style(text, { fg: readableForeground(theme.colors[segment.tone], bg), bg, bold: true });
      }
      const styled = theme.fg(segment.tone, text);
      return segment.bold ? theme.bold(styled) : styled;
    };
    const group = (side: string) => entries.filter((entry) => entry.side === side && entry.size > 0)
      .map((entry, index) => (index ? theme.fg("dim", before(entry.segment, side)) : "") + paint(entry.segment, fitLabel(entry.text, entry.size, entry.segment.clip) + " ".repeat(Math.max(0, entry.size - entry.natural))))
      .join("");
    const left = group("start");
    const right = group("end");
    return new HStack([
      { component: new Text(left, 0, 0), basis: visibleWidth(left), shrink: 0 },
      { component: new Text(left && right ? theme.fg("dim", groupSeparator) : "", 0, 0), basis: left && right ? groupGap : 0, shrink: 0, grow: this.model.layout === "inline" ? 0 : 1 },
      { component: new Text(right, 0, 0), basis: visibleWidth(right), shrink: 0 },
    ]).render(width).slice(0, 1).map((line) => " ".repeat(indent) + truncateToWidth(line, width));
  }
}
