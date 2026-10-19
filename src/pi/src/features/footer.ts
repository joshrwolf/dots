import { basename } from "node:path";
import type { ContextUsage, ExtensionContext, Theme } from "@earendil-works/pi-coding-agent";
import { truncateToWidth } from "@earendil-works/pi-tui";
import { PriorityRow, type Segment } from "../ui/row.ts";
import { statusText } from "../ui/text.ts";

type FooterFactory = NonNullable<Parameters<ExtensionContext["ui"]["setFooter"]>[0]>;
type FooterData = Parameters<FooterFactory>[2];

export function createFooter(ctx: ExtensionContext, theme: Theme, data: FooterData, requestRender: () => void) {
  let usageKey: string | undefined;
  let usage: ContextUsage | undefined;
  let disposed = false;
  const row = new PriorityRow();
  const unsubscribe = data.onBranchChange(requestRender);
  return {
    dispose() { if (!disposed) { disposed = true; unsubscribe(); } },
    invalidate() { row.invalidate(); },
    render(width: number): string[] {
      if (disposed || width <= 0) return [];
      const model = ctx.model;
      const key = JSON.stringify([ctx.sessionManager.getSessionId(), ctx.sessionManager.getLeafId(), model?.provider, model?.id, model?.contextWindow]);
      if (usageKey !== key) { usage = ctx.getContextUsage(); usageKey = key; }
      const percent = usage && usage.tokens !== null && typeof usage.percent === "number" && Number.isFinite(usage.percent) ? usage.percent : undefined;
      const branch = data.getGitBranch();
      const start: Segment[] = [{ text: basename(ctx.cwd) || ctx.cwd, tone: "toolTitle", priority: 40, shrink: "clip", clip: "middle", minWidth: 10 }];
      if (branch) start.push({ text: branch, tone: "dim", priority: 20, shrink: "omit" });
      const end: Segment[] = [];
      end.push({ text: model?.id ?? "no model", tone: "muted", priority: 50, shrink: "clip", clip: "middle", minWidth: 12 });
      if (ctx.thinkingLevel && ctx.thinkingLevel !== "off") end.push({ text: ctx.thinkingLevel, tone: "dim", priority: 10, shrink: "omit" });
      const context = percent === undefined ? "ctx ?" : `ctx ${Math.max(0, percent).toFixed(0)}%`;
      end.push({ text: context, minWidth: context.length, tone: percent !== undefined && percent >= 90 ? "error" : percent !== undefined && percent >= 75 ? "warning" : "dim", priority: 100, shrink: "clip" });
      row.update({ layout: "edges", start, end, separator: " · " }, theme);
      const lines = row.render(width);
      // setStatus requests a redraw without invalidating the footer. Never cache
      // this collection behind a leaf revision or component invalidate callback.
      const statuses = [...data.getExtensionStatuses()].sort(([a], [b]) => a.localeCompare(b)).map(([, text]) => statusText(text));
      if (statuses.length) lines.push(truncateToWidth(statuses.join(" "), width));
      return lines;
    },
  };
}
