import { basename } from "node:path";
import type { ContextUsage, ExtensionContext, Theme } from "@earendil-works/pi-coding-agent";
import { FooterView } from "./frame.ts";

type FooterFactory = NonNullable<Parameters<ExtensionContext["ui"]["setFooter"]>[0]>;
type FooterData = Parameters<FooterFactory>[2];

/** Host observation is separate from presentation; use Pi's branch provider,
 * never start a second Git poller or duplicate worker/job state here. */
export function createFooter(ctx: ExtensionContext, theme: Theme, data: FooterData, requestRender: () => void) {
  let usageKey: string | undefined;
  let usage: ContextUsage | undefined;
  let disposed = false;
  const view = new FooterView();
  const unsubscribe = data.onBranchChange(requestRender);
  return {
    dispose() { if (!disposed) { disposed = true; unsubscribe(); } },
    invalidate() { view.invalidate(); },
    render(width: number): string[] {
      if (disposed || width <= 0) return [];
      const model = ctx.model;
      const key = JSON.stringify([ctx.sessionManager.getSessionId(), ctx.sessionManager.getLeafId(), model?.provider, model?.id, model?.contextWindow]);
      if (usageKey !== key) { usage = ctx.getContextUsage(); usageKey = key; }
      view.update({ project: basename(ctx.cwd) || ctx.cwd, branch: data.getGitBranch() ?? undefined, model: model?.id ?? "no model", thinking: ctx.thinkingLevel,
        contextPercent: usage && usage.tokens !== null ? usage.percent ?? undefined : undefined,
        // setStatus redraws without invalidating. Read on each requested frame.
        statuses: [...data.getExtensionStatuses()].sort(([a], [b]) => a.localeCompare(b)).map(([, text]) => text),
      }, theme);
      return view.render(width);
    },
  };
}
