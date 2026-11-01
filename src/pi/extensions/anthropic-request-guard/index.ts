import {builtinProviders} from '@earendil-works/pi-ai/providers/all';
import {VERSION, type ExtensionAPI} from '@earendil-works/pi-coding-agent';
import {Diagnostics, ENTRY_TYPE, MAX_ENTRIES} from './diagnostics.ts';
import {guardProvider, type GuardOptions} from './guard.ts';

/** Deadline injection is for isolated host acceptance, not user configuration. */
export default function (
  pi: ExtensionAPI,
  options: Pick<GuardOptions, 'timeoutMs'> = {},
): void {
  let diagnostics: Diagnostics | undefined;
  let entries = 0;
  let dirty = false;
  const save = () => {
    if (!diagnostics || !dirty || entries >= MAX_ENTRIES) return;
    pi.appendEntry(ENTRY_TYPE, diagnostics.snapshot());
    entries++;
    dirty = false;
  };
  const anthropic = builtinProviders().find(
    provider => provider.id === 'anthropic',
  );
  if (!anthropic)
    throw new Error('Pi has no built-in Anthropic provider to guard');
  const guarded = guardProvider(anthropic, {
    ...options,
    observe(observation) {
      if (!diagnostics) return;
      diagnostics.record(observation);
      dirty = true;
      // Persist interventions immediately, even if a later retry or process crashes.
      if (observation.outcome === 'timeout') save();
    },
  });
  // Override streaming only; the host retains its remote catalog and auth layers.
  // Pi routes full calls here too: this registration uses SimpleStreamOptions.
  pi.registerProvider('anthropic', {
    api: 'anthropic-messages',
    streamSimple: guarded.streamSimple,
  });
  pi.on('session_start', (_event, ctx) => {
    diagnostics = new Diagnostics(VERSION);
    entries = ctx.sessionManager
      .getEntries()
      .filter(e => e.type === 'custom' && e.customType === ENTRY_TYPE).length;
    dirty = false;
  });
  pi.on('session_shutdown', () => {
    try {
      save();
    } finally {
      diagnostics = undefined;
    }
  });
  pi.registerCommand('request-guard', {
    description:
      'Inspect Anthropic startup guard timings and intervention counts for this runtime',
    handler: async (_args, ctx) => {
      const snapshot = diagnostics?.snapshot();
      ctx.ui.notify(
        snapshot
          ? `Anthropic startup guard: ${snapshot.timeoutMs / 1000}s after headers\n` +
              `This runtime: ${snapshot.counts.started} started, ${snapshot.counts.timeout} timed out, ${snapshot.counts.cancelled} cancelled, ${snapshot.counts.ended} ended before start\n` +
              `Start latency: mean ${snapshot.meanStartMs ?? '—'}ms, max ${snapshot.maxStartMs ?? '—'}ms\n` +
              `Session diagnostic entries: ${entries}/${MAX_ENTRIES}. ${entries >= MAX_ENTRIES ? 'Persistence cap reached; in-memory counters continue.' : 'Timeouts and shutdown summaries persist outside model context.'}`
          : 'Anthropic startup guard has no active session',
        'info',
      );
    },
  });
}
