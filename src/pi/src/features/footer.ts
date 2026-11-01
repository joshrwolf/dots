import {basename} from 'node:path';
import {
  SettingsManager,
  type ExtensionAPI,
  type ContextUsage,
  type ExtensionContext,
  type Theme,
} from '@earendil-works/pi-coding-agent';
import {FooterView} from './frame.ts';
import {compactionMeterLimit} from '../ui/context.ts';

type FooterFactory = NonNullable<
  Parameters<ExtensionContext['ui']['setFooter']>[0]
>;
type FooterData = Parameters<FooterFactory>[2];

/** Host observation is separate from presentation; use Pi's branch provider,
 * never start a second Git poller or duplicate worker/job state here. */
export function createFooter(
  ctx: ExtensionContext,
  theme: Theme,
  data: FooterData,
  requestRender: () => void,
  getSettings: ExtensionAPI['getSettings'],
) {
  let usageKey: string | undefined;
  let usage: ContextUsage | undefined;
  let compactLimit: number | undefined;
  let disposed = false;
  const view = new FooterView();
  const unsubscribe = data.onBranchChange(requestRender);
  return {
    dispose() {
      if (!disposed) {
        disposed = true;
        unsubscribe();
      }
    },
    invalidate() {
      view.invalidate();
    },
    render(width: number): string[] {
      if (disposed || width <= 0) return [];
      const model = ctx.model;
      const currentSettings = getSettings();
      const key = JSON.stringify([
        ctx.sessionManager.getSessionId(),
        ctx.sessionManager.getLeafId(),
        model?.provider,
        model?.id,
        model?.contextWindow,
        currentSettings.compaction,
      ]);
      if (usageKey !== key) {
        usage = ctx.getContextUsage();
        const settings = model
          ? SettingsManager.inMemory(currentSettings).getCompactionSettings(
              model,
            )
          : undefined;
        compactLimit = compactionMeterLimit(
          model ? `${model.provider}/${model.id}` : undefined,
          usage?.contextWindow ?? model?.contextWindow,
          settings?.enabled && model
            ? (usage?.contextWindow ?? model.contextWindow) -
                settings.reserveTokens
            : null,
        );
        usageKey = key;
      }
      view.update(
        {
          project: basename(ctx.cwd) || ctx.cwd,
          branch: data.getGitBranch() ?? undefined,
          model: model?.id ?? 'no model',
          thinking: ctx.thinkingLevel,
          contextPercent:
            usage && usage.tokens !== null
              ? compactLimit === undefined
                ? (usage.percent ?? undefined)
                : (usage.tokens / compactLimit) * 100
              : undefined,
          // setStatus redraws without invalidating. Read on each requested frame.
          statuses: [...data.getExtensionStatuses()].map(([key, text]) => ({
            key,
            text,
          })),
        },
        theme,
      );
      return view.render(width);
    },
  };
}
