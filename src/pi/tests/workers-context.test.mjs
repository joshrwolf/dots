import assert from 'node:assert/strict';
import test from 'node:test';
import {
  CONTEXT_MARK,
  contextLabel,
  decodeWorkerContext,
  registerWorkerContext,
  workerMeter,
  observedContext,
} from '../extensions/workers/context.ts';

function harness({
  provider = 'anthropic',
  window = 1_000_000,
  settings,
  entries = [],
} = {}) {
  const handlers = {};
  const reports = [];
  const model = {provider, id: 'fixture', contextWindow: window};
  const state = {
    tokens: 249_999,
    settings: settings ?? {
      compaction: {
        modelOverrides: {'anthropic/fixture': {reserveTokens: 700_000}},
      },
    },
  };
  const pi = {
    on: (event, handler) => {
      handlers[event] = handler;
    },
    getSettings: () => state.settings,
  };
  const ctx = {
    model,
    getContextUsage: () => ({tokens: state.tokens, contextWindow: window}),
    sessionManager: {getBranch: () => entries},
    ui: {notify: text => reports.push(decodeWorkerContext(text))},
  };
  registerWorkerContext(pi);
  const emit = (name, event = {}) => handlers[name]?.(event, ctx);
  emit('session_start');
  const turn = () =>
    emit('turn_end', {
      outcome: 'completed',
      context: {canContinue: true},
      entries: [],
    });
  return {state, reports, entries, emit, turn};
}

test('child reports its resolved budget each turn without adding entries', () => {
  const h = harness();
  assert.deepEqual(h.reports[0], {
    model: 'anthropic/fixture',
    tokens: 249_999,
    window: 1_000_000,
    compactAt: 300_000,
    compactions: 0,
  });
  h.state.tokens = 290_000;
  assert.equal(h.turn(), undefined, 'turns are observed, never extended');
  assert.equal(h.reports.at(-1).tokens, 290_000);
  assert.equal(h.entries.length, 0);
});

test('compactions count the active branch across compaction, reload, and tree navigation', () => {
  const h = harness({entries: [{type: 'message'}, {type: 'compaction'}]});
  assert.equal(h.reports[0].compactions, 1, 'resume counts earlier ones');
  h.entries.push({type: 'compaction'});
  h.state.tokens = null;
  h.emit('session_compact');
  assert.equal(h.reports.at(-1).tokens, null);
  assert.equal(h.reports.at(-1).compactions, 2);
  h.turn();
  assert.equal(h.reports.at(-1).compactions, 2);
  h.emit('session_compact_failed');
  h.turn();
  assert.equal(h.reports.at(-1).compactions, 2, 'failures are not compactions');
  h.entries.splice(1);
  h.emit('session_tree');
  assert.equal(h.reports.at(-1).compactions, 0);
  h.entries.push({type: 'compaction'});
  h.emit('session_start');
  assert.equal(h.reports.at(-1).compactions, 1);
});

test('OpenAI budgets use native defaults', () => {
  const h = harness({provider: 'openai-codex', window: 272_000});
  assert.equal(h.reports[0].compactAt, 272_000 - 16_384);
});

test('disabled and custom compaction settings are reported, not overridden', () => {
  for (const [compaction, expected] of [
    [{enabled: false}, null],
    [{reserveTokens: 600_000}, 400_000],
  ]) {
    const h = harness({settings: {compaction}});
    assert.equal(h.reports[0].compactAt, expected);
  }
});

test('descendant context retains observation age and hides stale meters', () => {
  const observation = {
    snapshot: {
      model: 'anthropic/fixture',
      tokens: 250_000,
      window: 1_000_000,
      compactAt: 300_000,
      compactions: 0,
    },
    observedAt: 1000,
  };
  const fresh = observedContext(observation, 2000);
  assert.equal(Math.round(fresh.context), 83);
  assert.match(fresh.text, /compacts at 300k.*observed 1s ago/);
  assert.doesNotMatch(fresh.text, /compacted/);
  assert.equal(fresh.compactions, 0);
  const compacted = observedContext(
    {...observation, snapshot: {...observation.snapshot, compactions: 3}},
    2000,
  );
  assert.match(compacted.text, /capacity · compacted 3× · observed 1s ago/);
  assert.equal(compacted.compactions, 3);
  const stale = observedContext(observation, 91_000);
  assert.equal(stale.context, undefined);
  assert.match(stale.text, /observed 90s ago \(stale\)/);
  assert.equal(observedContext(observation, 2000, true).context, undefined);
  assert.equal(observedContext(observation, 0).context, undefined);
  assert.match(observedContext(undefined, 2000).text, /unavailable/);
  assert.equal(
    observedContext(
      {...observation, snapshot: {...observation.snapshot, tokens: null}},
      2000,
    ).context,
    undefined,
  );
});

test('context channel rejects malformed and nonfinite facts', () => {
  const valid = {
    model: 'anthropic/fixture',
    tokens: 250_000,
    window: 1_000_000,
    compactAt: 300_000,
    compactions: 2,
  };
  assert.deepEqual(
    decodeWorkerContext(CONTEXT_MARK + JSON.stringify(valid)),
    valid,
  );
  for (const value of [
    {...valid, tokens: -1},
    {...valid, window: 1.5},
    {...valid, compactions: -1},
    {...valid, compactions: undefined},
    {...valid, pressure: true},
    {...valid, compactAt: '300K'},
    {...valid, extra: true},
  ]) {
    assert.equal(
      decodeWorkerContext(CONTEXT_MARK + JSON.stringify(value)),
      undefined,
    );
  }
  assert.equal(decodeWorkerContext(CONTEXT_MARK + '{'), undefined);
  assert.equal(decodeWorkerContext('ordinary notification'), undefined);
});

test('worker meters use Anthropic compaction budgets without changing physical capacity', () => {
  const worker = {
    model: 'anthropic/fixture',
    contextTokens: 250_000,
    contextWindow: 1_000_000,
    compactAt: 300_000,
  };
  const meter = workerMeter(worker);
  assert.equal(Math.round(meter.context), 83);
  assert.equal(meter.contextBasis, 'compact');
  assert.equal(worker.contextWindow, 1_000_000);
  assert.equal(workerMeter({...worker, contextTokens: 300_000}).context, 100);
  assert.equal(
    workerMeter({...worker, contextTokens: null}).context,
    undefined,
  );
  assert.deepEqual(workerMeter({...worker, model: 'openai-codex/fixture'}), {
    context: 25,
    contextBasis: undefined,
  });
  assert.deepEqual(workerMeter({...worker, compactAt: null}), {
    context: 25,
    contextBasis: undefined,
  });
  assert.equal(
    workerMeter({...worker, contextWindow: 200_000}).contextBasis,
    undefined,
  );
});

test('budget labels distinguish usage, trigger, capacity, disabled, and unknown', () => {
  assert.equal(
    contextLabel({
      contextTokens: 245_000,
      compactAt: 300_000,
      contextWindow: 1_000_000,
    }),
    '~245k context · compacts at 300k · 1.0M capacity',
  );
  assert.match(
    contextLabel({contextTokens: null, compactAt: null}),
    /unknown context · auto-compaction off/,
  );
  assert.match(contextLabel({}), /compaction unknown/);
});

test('budget labels count compactions once any happened', () => {
  const worker = {contextTokens: 80_000, compactAt: 300_000};
  assert.doesNotMatch(contextLabel({...worker, compactions: 0}), /compacted/);
  assert.match(
    contextLabel({...worker, compactions: 2}),
    /capacity unknown · compacted 2×$/,
  );
});
