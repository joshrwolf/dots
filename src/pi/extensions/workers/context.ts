import {
  SettingsManager,
  type ExtensionAPI,
  type ExtensionContext,
} from '@earendil-works/pi-coding-agent';
import {Type, type Static} from 'typebox';
import {Check} from 'typebox/value';
import {compact, STALL_MS} from './present.ts';
import {compactionMeterLimit} from '../../src/ui/context.ts';

/** Child-reported facts: its project settings and current model may differ from its parent's. */
export const CONTEXT_MARK = 'dots.worker.context:';
const PRESSURE_ENTRY = 'dots.worker.context-pressure';
const count = Type.Integer({minimum: 0, maximum: Number.MAX_SAFE_INTEGER});
export const WorkerContextSchema = Type.Object(
  {
    model: Type.String({minLength: 1, maxLength: 512}),
    tokens: Type.Union([count, Type.Null()]),
    window: count,
    /** Null means native automatic compaction is disabled. */
    compactAt: Type.Union([count, Type.Null()]),
    pressure: Type.Boolean(),
  },
  {additionalProperties: false},
);
export type WorkerContext = Static<typeof WorkerContextSchema>;

export function decodeWorkerContext(text: string): WorkerContext | undefined {
  if (!text.startsWith(CONTEXT_MARK)) return undefined;
  let value: unknown;
  try {
    value = JSON.parse(text.slice(CONTEXT_MARK.length));
  } catch {
    return undefined;
  }
  return Check(WorkerContextSchema, value) ? value : undefined;
}

/** Presentation-only percentage; status details retain the physical-window percentage. */
export function workerMeter(worker: {
  model?: string | undefined;
  contextTokens?: number | null | undefined;
  contextWindow?: number | undefined;
  compactAt?: number | null | undefined;
}): {context: number | undefined; contextBasis: 'compact' | undefined} {
  const limit = compactionMeterLimit(
    worker.model,
    worker.contextWindow,
    worker.compactAt,
  );
  const denominator = limit ?? worker.contextWindow;
  return {
    context:
      typeof worker.contextTokens === 'number' && denominator
        ? (worker.contextTokens / denominator) * 100
        : undefined,
    contextBasis: limit === undefined ? undefined : 'compact',
  };
}

export function contextLabel(worker: {
  contextTokens?: number | null | undefined;
  contextWindow?: number | undefined;
  compactAt?: number | null | undefined;
}): string {
  const tokens =
    typeof worker.contextTokens === 'number'
      ? `~${compact(worker.contextTokens)}`
      : 'unknown';
  const budget =
    worker.compactAt === undefined
      ? 'compaction unknown'
      : worker.compactAt === null
        ? 'auto-compaction off'
        : `compacts at ${compact(worker.compactAt)}`;
  const capacity =
    worker.contextWindow === undefined
      ? 'capacity unknown'
      : `${compact(worker.contextWindow)} capacity`;
  return `${tokens} context · ${budget} · ${capacity}`;
}

export function observedContext(
  observation: {snapshot: WorkerContext; observedAt: number} | undefined,
  now: number,
  ownerLost = false,
): {
  text: string;
  context: number | undefined;
  contextBasis: 'compact' | undefined;
} {
  if (!observation)
    return {
      text: 'context unavailable',
      context: undefined,
      contextBasis: undefined,
    };
  const {snapshot, observedAt} = observation;
  const age = Math.max(0, now - observedAt);
  const stale = ownerLost || observedAt > now || age >= STALL_MS;
  const worker = {
    model: snapshot.model,
    contextTokens: snapshot.tokens,
    contextWindow: snapshot.window,
    compactAt: snapshot.compactAt,
  };
  return {
    text: `${contextLabel(worker)} · observed ${Math.floor(age / 1000)}s ago${stale ? ' (stale)' : ''}`,
    ...workerMeter({...worker, contextTokens: stale ? null : snapshot.tokens}),
  };
}

export function pressureMessage(context: WorkerContext): string {
  return `Context pressure: approximately ${context.tokens === null ? 'unknown' : compact(context.tokens)} tokens; native compaction at ${context.compactAt === null ? 'disabled' : compact(context.compactAt)} (${compact(context.window)} capacity). Finish the current bounded step and checkpoint completed work, remaining work, decisions, files/artifacts, checks run, and unresolved risks. Keep substantial detail in an artifact and report its path. The parent chooses whether to finish, compact and continue, or stop and replace; this is advisory, not a failure or an instruction to spawn a replacement. Stop the old worker before transferring edit ownership; do not copy its full transcript into a fresh worker.`;
}

/** Runs only in managed children. Pi owns compaction; this observes it and adds advisory context. */
export function registerWorkerContext(pi: ExtensionAPI): void {
  let warned = false;
  const restore = (ctx: ExtensionContext) => {
    warned = false;
    for (const entry of [...ctx.sessionManager.getBranch()].reverse()) {
      if (entry.type === 'compaction') break;
      if (
        entry.type === 'custom_message' &&
        entry.customType === PRESSURE_ENTRY
      ) {
        warned = true;
        break;
      }
    }
  };
  const snapshot = (ctx: ExtensionContext): WorkerContext | undefined => {
    const model = ctx.model;
    const usage = ctx.getContextUsage();
    if (!model || !usage) return undefined;
    // Use Pi's own model override resolution and validation, not a second settings implementation.
    const settings = SettingsManager.inMemory(
      pi.getSettings(),
    ).getCompactionSettings(model);
    return {
      model: `${model.provider}/${model.id}`,
      tokens: usage.tokens,
      window: usage.contextWindow,
      compactAt: settings.enabled
        ? Math.max(0, usage.contextWindow - settings.reserveTokens)
        : null,
      pressure: false,
    };
  };
  const report = (ctx: ExtensionContext, context: WorkerContext) => {
    ctx.ui.notify(CONTEXT_MARK + JSON.stringify(context), 'info');
  };
  pi.on('session_start', (_event, ctx) => {
    restore(ctx);
    const context = snapshot(ctx);
    if (context) report(ctx, context);
  });
  pi.on('session_tree', (_event, ctx) => {
    restore(ctx);
    const context = snapshot(ctx);
    if (context) report(ctx, context);
  });
  pi.on('model_select', (_event, ctx) => {
    const context = snapshot(ctx);
    if (context) report(ctx, context);
  });
  pi.on('session_compact', (_event, ctx) => {
    warned = false;
    const context = snapshot(ctx);
    if (context) report(ctx, context);
  });
  pi.on('turn_end', (event, ctx) => {
    const context = snapshot(ctx);
    if (!context) return;
    // Leave 50K for checkpointing before native compaction. Only Anthropic 1M
    // workers opt in; disabled compaction and OpenAI retain their behavior.
    const pressure =
      !warned &&
      event.outcome === 'completed' &&
      event.context.canContinue &&
      ctx.model?.provider === 'anthropic' &&
      context.window >= 1_000_000 &&
      context.compactAt !== null &&
      context.compactAt > 50_000 &&
      context.tokens !== null &&
      context.tokens >= context.compactAt - 50_000;
    report(ctx, {...context, pressure});
    if (!pressure) return;
    warned = true;
    return {
      entries: [
        ...event.entries,
        {
          type: 'custom_message',
          customType: PRESSURE_ENTRY,
          content: pressureMessage(context),
          display: false,
        },
      ],
    };
  });
}
