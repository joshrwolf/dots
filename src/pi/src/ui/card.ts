import { displayLimits } from "./policy.ts";
import type { Theme } from "@earendil-works/pi-coding-agent";
import { VStack, stripTerminalSequences, visibleWidth, type Component } from "@earendil-works/pi-tui";
import { PriorityRow, type RowModel, type Segment } from "./row.ts";
import { StateGutter, type ActivityState } from "./state.ts";
import { DetailView } from "./details.ts";
import { labelText, layoutText, plainLines, sourceText } from "./text.ts";

export interface Notice { readonly text: string; readonly severity: "warning" | "error" | "dim" }

export class Notices implements Component {
  private theme: Theme | undefined;
  private values: readonly Notice[] = [];
  update(values: readonly Notice[], theme: Theme): void {
    this.values = values; this.theme = theme;
  }
  invalidate(): void {}
  render(width: number): string[] {
    const theme = this.theme;
    if (!theme || width <= 0) return [];
    const inset = width >= 20 ? "  " : "";
    const room = width - inset.length;
    return this.values.flatMap((notice) => {
      const mark = room >= 8 ? notice.severity === "error" ? "↳ " : notice.severity === "warning" ? "! " : "· " : "";
      return plainLines(notice.text, room - mark.length).map((line, index) => inset + theme.fg(notice.severity, (index === 0 ? mark : " ".repeat(mark.length)) + line));
    });
  }
}

/** Bounded recorded output, with tail diagnostics retained when the head is cut. */
export class OutputView implements Component {
  private readonly text: string;
  private cache: { width: number; lines: string[] } | undefined;
  constructor(text: string) {
    this.text = sourceText(text.length <= displayLimits.sourceCodeUnits ? text : text.slice(0, displayLimits.sourceCodeUnits / 2) + "\n… output display limited …\n" + text.slice(-displayLimits.sourceCodeUnits / 2));
  }
  invalidate(): void { this.cache = undefined; }
  render(width: number): string[] {
    if (width <= 0) return [];
    if (this.cache?.width === width) return this.cache.lines;
    const prepared = layoutText(this.text, width, displayLimits.outputRows);
    let lines = prepared.lines;
    if (prepared.limited) {
      // Work is bounded before wrapping. The suffix is intentionally small so
      // even one-column output retains the final diagnostic.
      const tail = plainLines(this.text.slice(-Math.max(1, Math.min(displayLimits.outputRows, width * 20))), width, displayLimits.outputRows).slice(-Math.floor(displayLimits.outputRows / 4));
      const marker = lines.at(-1)!;
      lines = [...lines.slice(0, displayLimits.outputRows - tail.length - 1), marker, ...tail];
    }
    this.cache = { width, lines };
    return lines;
  }
}

/** A bounded glance at recorded text. Expansion always uses the full OutputView. */
export class PreviewView implements Component {
  private readonly output: OutputView;
  private readonly theme: Theme;
  private readonly rows: number;
  constructor(text: string, theme: Theme, rows = 3) { this.output = new OutputView(text); this.theme = theme; this.rows = rows; }
  invalidate(): void { this.output.invalidate(); }
  render(width: number): string[] {
    if (width <= 0) return [];
    const lines = this.output.render(width);
    return lines.length <= this.rows ? lines : [...lines.slice(0, this.rows), ...plainLines("… Ctrl+O to expand", width, 1).map((line) => this.theme.fg("dim", line))];
  }
}

export interface ActivityCardModel {
  /** Optional static decoration; the caller supplies the semantic state. */
  readonly state?: ActivityState;
  readonly notices: readonly Notice[];
  readonly body: Component | undefined;
  readonly caption: string;
  /** Default always; omit a redundant caption only when its full text appears in the rendered header. */
  readonly captionPolicy?: "always" | "when-not-in-header";
  readonly header: RowModel;
}

/** Outcomes stay beside the subject; only explicitly secondary metadata spans
 * to the trailing edge. Shrink priorities still protect outcomes in narrow panes. */
export function activityHeader(kind: string, subject: string, status: readonly Segment[], clip: NonNullable<Segment["clip"]> = "middle", secondary: readonly Segment[] = []): RowModel {
  return {
    layout: "edges", separator: "  ", groupSeparator: "  ", endSeparator: " · ",
    start: [
      { text: kind, tone: "accent", bold: true, priority: 90, shrink: "clip", padTo: 8, minWidth: Math.min(5, visibleWidth(kind)) },
      { text: subject, tone: "text", priority: 40, shrink: "clip", clip, minWidth: 8 },
      ...status.map((segment, index) => ({ ...segment, separatorBefore: segment.separatorBefore ?? (index === 0 ? "  " : " · ") })),
    ], end: secondary,
  };
}

/** Composition only: no tool/session state or syntax service ownership. */
export class ActivityCard extends VStack {
  private readonly row = new PriorityRow();
  private readonly notices = new Notices();
  update(model: ActivityCardModel, theme: Theme): void {
    this.row.update(model.header, theme);
    this.notices.update(model.notices, theme);
    this.clear();
    const decorated = new StateGutter(this.row, model.state ?? "done", theme);
    this.addChild(decorated);
    this.addChild(this.notices);
    if (model.body) {
      const caption = model.captionPolicy === "when-not-in-header"
        ? (width: number) => stripTerminalSequences(decorated.render(width).join("")).includes(labelText(model.caption)) ? "" : model.caption
        : model.caption;
      this.addChild(new DetailView(model.body, caption, theme));
    }
  }
  override render(width: number): string[] { return width > 0 ? super.render(width) : []; }
}
