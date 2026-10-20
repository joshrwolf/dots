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
    const start: Segment[] = [{ text: model.project, tone: "accent", priority: 40, shrink: "clip", clip: "middle", minWidth: 10 }];
    if (model.branch) start.push({ text: `⑂ ${model.branch}`, tone: "muted", priority: 20, shrink: "omit" });
    const end: Segment[] = [{ text: model.model || "no model", tone: "text", priority: 50, shrink: "clip", clip: "middle", minWidth: 12 }];
    if (model.thinking && model.thinking !== "off") end.push({ text: `✦ ${model.thinking}`, tone: "muted", priority: 10, shrink: "omit" });
    const context = known ? `ctx ${Math.max(0, percent).toFixed(0)}%` : "ctx ?";
    const warning = known && percent >= 75;
    const tone = known && percent >= 90 ? "error" : warning ? "warning" : "muted";
    // The context value survives even when its decorative icon cannot fit.
    end.push({ text: warning ? "!" : "◔", tone, priority: 90, shrink: "omit" });
    end.push({ text: context, minWidth: context.length, tone, priority: 100, shrink: "clip", separatorBefore: " " });
    this.row.update({ layout: "edges", start, end, separator: "  " }, theme);
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
