import { displayLimits } from "./policy.ts";
import type { Theme } from "@earendil-works/pi-coding-agent";
import { VStack, stripTerminalSequences, type Component } from "@earendil-works/pi-tui";
import { PriorityRow, type RowLayout, type RowModel, type Segment } from "./row.ts";
import { StateGutter, type ActivityState } from "./state.ts";
import { DetailView } from "./details.ts";
import { labelText, layoutText, plainLines, sourceText } from "./text.ts";

export interface Notice { readonly text: string; readonly severity: "warning" | "error" | "dim" }

export class Notices implements Component {
  private theme: Theme | undefined;
  private values: readonly Notice[] = [];
  private indent = 0;
  update(values: readonly Notice[], theme: Theme, indent = 0): void {
    this.values = values; this.theme = theme;
    this.indent = Number.isFinite(indent) ? Math.max(0, Math.floor(indent)) : 0;
  }
  invalidate(): void {}
  render(width: number): string[] {
    const theme = this.theme;
    if (!theme || width <= 0) return [];
    const indent = Math.min(this.indent, Math.max(0, width - 16));
    return this.values.flatMap((notice) => plainLines(notice.text, width - indent).map((line) => " ".repeat(indent) + theme.fg(notice.severity, line)));
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

export type ToolCardModel = {
  /** Optional static decoration; the caller supplies the semantic state. */
  readonly state?: ActivityState;
  readonly notices: readonly Notice[];
  /** Soft indentation, surrendered to retain up to 16 cells for diagnostics. */
  readonly noticeIndent?: number;
  readonly body: Component | undefined;
  readonly caption: string;
  /** Default always; omit a redundant caption only when its full text appears in the rendered header. */
  readonly captionPolicy?: "always" | "when-not-in-header";
} & ({
  /** Explicit row composition; caller owns order, grouping and spacing. */
  readonly header: RowModel;
  readonly heading?: never;
  readonly status?: never;
  readonly layout?: never;
  readonly groupSeparator?: never;
} | {
  readonly header?: never;
  readonly heading: readonly Segment[];
  readonly status: readonly Segment[];
  /** Header placement only; defaults to inline and never constrains the body. */
  readonly layout?: RowLayout;
  readonly groupSeparator?: string;
});

/** Composition only: no tool/session state or syntax service ownership. */
export class ToolCard implements Component {
  private readonly row = new PriorityRow();
  private readonly notices = new Notices();
  private readonly stack = new VStack();
  update(model: ToolCardModel, theme: Theme): void {
    const header = model.header ?? { layout: model.layout, start: model.heading, end: model.status, endSeparator: " · ", ...(model.groupSeparator === undefined ? {} : { groupSeparator: model.groupSeparator }) };
    this.row.update({ ...header, layout: header.layout ?? "inline" }, theme);
    this.notices.update(model.notices, theme, model.noticeIndent);
    this.stack.clear();
    const decorated = model.state ? new StateGutter(this.row, model.state, theme) : this.row;
    this.stack.addChild(decorated);
    this.stack.addChild(this.notices);
    if (model.body) {
      const caption = model.captionPolicy === "when-not-in-header"
        ? (width: number) => stripTerminalSequences(decorated.render(width).join("")).includes(labelText(model.caption)) ? "" : model.caption
        : model.caption;
      this.stack.addChild(new DetailView(model.body, caption, theme));
    }
  }
  invalidate(): void { this.stack.invalidate(); }
  render(width: number): string[] { return width > 0 ? this.stack.render(width) : []; }
}
