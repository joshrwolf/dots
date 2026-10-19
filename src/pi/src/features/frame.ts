import type { Theme } from "@earendil-works/pi-coding-agent";
import { truncateToWidth, type Component } from "@earendil-works/pi-tui";
import { PriorityRow, type Segment } from "../ui/row.ts";
import { statusText } from "../ui/text.ts";

export interface FooterModel {
  readonly project: string;
  readonly branch?: string | undefined;
  readonly model: string;
  readonly thinking?: string | undefined;
  readonly contextPercent?: number | undefined;
  readonly statuses: readonly string[];
}

/** One source for model/context information; no clocks, probes or session reads. */
export class FooterView implements Component {
  private readonly row = new PriorityRow();
  private statuses: readonly string[] = [];
  update(model: FooterModel, theme: Theme): void {
    const percent = model.contextPercent;
    const known = typeof percent === "number" && Number.isFinite(percent);
    const start: Segment[] = [{ text: model.project, tone: "toolTitle", bold: true, priority: 40, shrink: "clip", clip: "middle", minWidth: 10 }];
    if (model.branch) start.push({ text: model.branch, tone: "dim", priority: 20, shrink: "omit" });
    const end: Segment[] = [{ text: model.model || "no model", tone: "muted", priority: 50, shrink: "clip", clip: "middle", minWidth: 12 }];
    if (model.thinking && model.thinking !== "off") end.push({ text: `think ${model.thinking}`, tone: "dim", priority: 10, shrink: "omit" });
    const context = known ? `ctx ${Math.max(0, percent).toFixed(0)}%` : "ctx ?";
    end.push({ text: context, minWidth: context.length, tone: known && percent >= 90 ? "error" : known && percent >= 75 ? "warning" : "dim", priority: 100, shrink: "clip" });
    this.row.update({ layout: "edges", start, end, separator: " · " }, theme);
    this.statuses = model.statuses.map(statusText);
  }
  invalidate(): void { this.row.invalidate(); }
  render(width: number): string[] {
    if (width <= 0) return [];
    const lines = this.row.render(width);
    if (this.statuses.length) lines.push(truncateToWidth(this.statuses.join(" "), width));
    return lines;
  }
}

/** Startup chrome only: model, context and active work belong below the editor. */
export function createHeader(theme: Theme): Component {
  const row = new PriorityRow();
  row.update({ layout: "inline", groupSeparator: "  ", start: [
    { text: "Pi", tone: "accent", bold: true, priority: 100, shrink: "clip" },
  ], end: [{ text: "/ui appearance", tone: "dim", priority: 10, shrink: "omit" }] }, theme);
  return row;
}
