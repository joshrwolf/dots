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
const count = Type.Integer({minimum: 0, maximum: Number.MAX_SAFE_INTEGER});
export const WorkerContextSchema = Type.Object(
  {
    model: Type.String({minLength: 1, maxLength: 512}),
    tokens: Type.Union([count, Type.Null()]),
    window: count,
    /** Null means native automatic compaction is disabled. */
    compactAt: Type.Union([count, Type.Null()]),
    /** Compactions on the child's active session branch. */
    compactions: count,
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
  compactions?: number | undefined;
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
  const compacted = worker.compactions
    ? ` · compacted ${worker.compactions}×`
    : '';
  return `${tokens} context · ${budget} · ${capacity}${compacted}`;
}

export function observedContext(
  observation: {snapshot: WorkerContext; observedAt: number} | undefined,
  now: number,
  ownerLost = false,
): {
  text: string;
  context: number | undefined;
  contextBasis: 'compact' | undefined;
  compactions: number | undefined;
} {
  if (!observation)
    return {
      text: 'context unavailable',
      context: undefined,
      contextBasis: undefined,
      compactions: undefined,
    };
  const {snapshot, observedAt} = observation;
  const age = Math.max(0, now - observedAt);
  const stale = ownerLost || observedAt > now || age >= STALL_MS;
  const worker = {
    model: snapshot.model,
    contextTokens: snapshot.tokens,
    contextWindow: snapshot.window,
    compactAt: snapshot.compactAt,
    compactions: snapshot.compactions,
  };
  return {
    text: `${contextLabel(worker)} · observed ${Math.floor(age / 1000)}s ago${stale ? ' (stale)' : ''}`,
    ...workerMeter({...worker, contextTokens: stale ? null : snapshot.tokens}),
    compactions: snapshot.compactions,
  };
}

/** Runs only in managed children. Pi owns compaction; this reports its budget and history. */
export function registerWorkerContext(pi: ExtensionAPI): void {
  let compactions = 0;
  const recount = (ctx: ExtensionContext) => {
    compactions = ctx.sessionManager
      .getBranch()
      .filter(entry => entry.type === 'compaction').length;
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
      compactions,
    };
  };
  const report = (ctx: ExtensionContext, context: WorkerContext) => {
    ctx.ui.notify(CONTEXT_MARK + JSON.stringify(context), 'info');
  };
  const update = (ctx: ExtensionContext) => {
    const context = snapshot(ctx);
    if (context) report(ctx, context);
  };
  // Only these change the active branch's compactions; turns reuse the count.
  const rebranch = (_event: unknown, ctx: ExtensionContext) => {
    recount(ctx);
    update(ctx);
  };
  pi.on('session_start', rebranch);
  pi.on('session_tree', rebranch);
  pi.on('session_compact', rebranch);
  pi.on('model_select', (_event, ctx) => update(ctx));
  pi.on('turn_end', (_event, ctx) => update(ctx));
}
