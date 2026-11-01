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
  const turn = (overrides = {}) => {
    const result = emit('turn_end', {
      outcome: 'completed',
      context: {canContinue: true},
      entries: [],
      ...overrides,
    });
    if (result?.entries) entries.push(...result.entries);
    return result;
  };
  return {state, reports, entries, emit, turn};
}

test('child reports its resolved model budget; warns both sides once at 250K', () => {
  const h = harness();
  assert.equal(h.reports[0].compactAt, 300_000);
  assert.equal(h.turn(), undefined);
  h.state.tokens = 250_000;
  const prior = {
    type: 'custom',
    customType: 'other-extension',
    data: {keep: true},
  };
  const result = h.turn({entries: [prior]});
  assert.equal(result.entries[0], prior, 'preserve earlier boundary handlers');
  assert.match(result.entries[1].content, /checkpoint completed work/);
  assert.equal(result.continue, undefined, 'never force another model request');
  assert.equal(h.reports.at(-1).pressure, true);
  h.state.tokens = 290_000;
  assert.equal(h.turn(), undefined);
  h.emit('session_start');
  assert.equal(h.turn(), undefined, 'reload does not repeat pressure');
  assert.equal(h.reports.filter(r => r.pressure).length, 1);
});

test('compaction rearms warning, reports unknown usage, and restores across reload', () => {
  const h = harness();
  h.state.tokens = 260_000;
  h.turn();
  h.entries.push({type: 'compaction'});
  h.state.tokens = null;
  h.emit('session_compact');
  assert.equal(h.reports.at(-1).tokens, null);
  assert.equal(h.turn(), undefined);
  h.emit('session_start');
  h.state.tokens = 250_000;
  assert.ok(h.turn());
  assert.equal(h.reports.filter(r => r.pressure).length, 2);
});

test('failed compaction and fluctuations alone do not repeat a warning', () => {
  const h = harness();
  h.state.tokens = 260_000;
  h.turn();
  h.emit('session_compact_failed');
  h.state.tokens = 200_000;
  h.turn();
  h.state.tokens = 260_000;
  assert.equal(h.turn(), undefined);
});

test('tree navigation reconstructs warning state on the active branch', () => {
  const h = harness();
  h.state.tokens = 260_000;
  h.turn();
  h.entries.length = 0;
  h.emit('session_tree');
  assert.ok(h.turn());
});

for (const outcome of ['aborted', 'error']) {
  test(`${outcome} turns do not ask the worker to continue`, () => {
    const h = harness();
    h.state.tokens = 290_000;
    assert.equal(h.turn({outcome}), undefined);
    assert.equal(h.reports.at(-1).pressure, false);
  });
}

test('final answers get telemetry without a warning or another turn', () => {
  const h = harness();
  h.state.tokens = 290_000;
  assert.equal(h.turn({context: {canContinue: false}}), undefined);
  assert.equal(h.reports.at(-1).pressure, false);
});

test('OpenAI budgets use native defaults and never receive Anthropic warnings', () => {
  const h = harness({provider: 'openai-codex', window: 272_000});
  assert.equal(h.reports[0].compactAt, 272_000 - 16_384);
  h.state.tokens = 260_000;
  assert.equal(h.turn(), undefined);
});

test('disabled and custom compaction settings are reported, not overridden', () => {
  for (const [compaction, expected] of [
    [{enabled: false}, null],
    [{reserveTokens: 600_000}, 400_000],
  ]) {
    const h = harness({settings: {compaction}});
    assert.equal(h.reports[0].compactAt, expected);
    h.state.tokens = 260_000;
    assert.equal(h.turn(), undefined);
  }
});

test('warning follows effective trigger with 50K headroom instead of an exact target', () => {
  const h = harness({settings: {compaction: {reserveTokens: 650_000}}});
  h.state.tokens = 299_999;
  assert.equal(h.turn(), undefined);
  h.state.tokens = 300_000;
  assert.ok(h.turn());
  assert.equal(h.reports.at(-1).compactAt, 350_000);
  assert.equal(h.reports.at(-1).pressure, true);
});

test('descendant context retains observation age and hides stale meters', () => {
  const observation = {
    snapshot: {
      model: 'anthropic/fixture',
      tokens: 250_000,
      window: 1_000_000,
      compactAt: 300_000,
      pressure: false,
    },
    observedAt: 1000,
  };
  const fresh = observedContext(observation, 2000);
  assert.equal(Math.round(fresh.context), 83);
  assert.match(fresh.text, /compacts at 300k.*observed 1s ago/);
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
    pressure: true,
  };
  assert.deepEqual(
    decodeWorkerContext(CONTEXT_MARK + JSON.stringify(valid)),
    valid,
  );
  for (const value of [
    {...valid, tokens: -1},
    {...valid, window: 1.5},
    {...valid, pressure: 'yes'},
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
