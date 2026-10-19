import type { Theme } from "@earendil-works/pi-coding-agent";
import { sliceByColumn, stripTerminalSequences, truncateToWidth, visibleWidth, type Component } from "@earendil-works/pi-tui";
import { PriorityRow, type RowModel } from "./row.ts";

export interface PanelModel {
  readonly heading: RowModel;
  readonly body: Component;
  readonly footer?: RowModel;
}

/** Light frame for read-only dashboards. Interactive scrolling stays outside the
 * panel, with the host/Inspector; this component owns no input or terminal state. */
export class Panel implements Component {
  private readonly heading = new PriorityRow();
  private readonly footer = new PriorityRow();
  private model: PanelModel | undefined;
  private theme: Theme | undefined;

  update(model: PanelModel, theme: Theme): void {
    this.model = model; this.theme = theme;
    this.heading.update(model.heading, theme);
    this.footer.update(model.footer ?? { start: [], end: [] }, theme);
    this.invalidate();
  }
  invalidate(): void { this.heading.invalidate(); this.footer.invalidate(); this.model?.body.invalidate(); }
  render(width: number): string[] {
    const model = this.model, theme = this.theme;
    if (!model || !theme || width <= 0) return [];
    // At very narrow widths, spend the cells on content rather than decoration.
    if (width < 12) return [...this.heading.render(width), ...model.body.render(width), ...this.footer.render(width)];
    const inner = width - 4;
    const border = (text: string) => theme.fg("borderMuted", text);
    const edge = (row: PriorityRow, left: string, right: string): string => {
      const line = row.render(inner - 1)[0] ?? "";
      const cells = visibleWidth(stripTerminalSequences(line).trimEnd());
      const label = sliceByColumn(line, 0, cells, true);
      return border(`${left}─`) + (cells ? ` ${label} ` : "──") + border("─".repeat(Math.max(0, inner - 1 - cells)) + right);
    };
    const body = model.body.render(inner).map((line) => {
      const bounded = truncateToWidth(line, inner, "");
      return border("│ ") + bounded + " ".repeat(Math.max(0, inner - visibleWidth(bounded))) + border(" │");
    });
    return [edge(this.heading, "╭", "╮"), ...body, edge(this.footer, "╰", "╯")].map((line) => truncateToWidth(line, width, ""));
  }
}
