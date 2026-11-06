import assert from 'node:assert/strict';
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  readFileSync,
  existsSync,
  rmSync,
} from 'node:fs';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {tmpdir} from 'node:os';
import {setTimeout as delay} from 'node:timers/promises';
import test from 'node:test';
import {PiProcess} from '../../extensions/workers/rpc.ts';
import {anthropicHttp} from '../fixtures/anthropic-http.mjs';

const extension = fileURLToPath(
  new URL('../fixtures/guard-worker.ts', import.meta.url),
);
async function until(predicate, timeout = 15000) {
  const end = Date.now() + timeout;
  for (;;) {
    const value = await predicate();
    if (value) return value;
    if (Date.now() > end) throw new Error('worker fixture deadline');
    await delay(10);
  }
}
async function setup(
  t,
  plan,
  compaction = {enabled: false, keepRecentTokens: 0},
) {
  const dir = mkdtempSync(join(tmpdir(), 'dots-guard-workers-'));
  const agent = join(dir, 'agent');
  mkdirSync(agent);
  const marker = join(dir, 'events.jsonl');
  writeFileSync(join(dir, 'fixture.txt'), 'neutral fixture content');
  const http = await anthropicHttp(plan);
  writeFileSync(
    join(agent, 'guard-fixture.json'),
    JSON.stringify({url: http.url, marker}),
  );
  writeFileSync(
    join(agent, 'auth.json'),
    JSON.stringify({anthropic: {type: 'api_key', key: 'fixture'}}),
  );
  writeFileSync(
    join(agent, 'settings.json'),
    JSON.stringify({
      extensions: [extension],
      defaultProvider: 'anthropic',
      defaultModel: 'claude-opus-5-5',
      defaultThinkingLevel: 'off',
      cacheWarming: 'off',
      compaction,
      retry: {
        enabled: true,
        maxRetries: 1,
        baseDelayMs: 300,
        maxAgentDelayMs: 300,
      },
    }),
  );
  const env = {
    PI_CODING_AGENT_DIR: agent,
    PI_OFFLINE: '1',
    ANTHROPIC_API_KEY: 'fixture',
    DOTS_GUARD_FIXTURE_URL: http.url,
    DOTS_GUARD_FIXTURE_MARKER: marker,
  };
  for (const key of Object.keys(process.env))
    if (
      /TOKEN|API_KEY|SECRET|PASSWORD/.test(key) ||
      key.startsWith('DOTS_WORKER_') ||
      key.startsWith('REX_')
    )
      env[key] = undefined;
  env.ANTHROPIC_API_KEY = 'fixture';
  const parent = new PiProcess({
    cwd: dir,
    args: [
      '--no-mcp',
      '--no-context-files',
      '--no-skills',
      '--no-prompt-templates',
      '--no-themes',
      '--no-approve',
      '--offline',
    ],
    env,
  });
  const records = () =>
    existsSync(marker)
      ? readFileSync(marker, 'utf8')
          .trim()
          .split('\n')
          .filter(Boolean)
          .map(JSON.parse)
      : [];
  let sequence = 0;
  const run = async (name, params) => {
    const id = String(++sequence);
    await parent.prompt('/fixture-call ' + JSON.stringify({id, name, params}));
    const record = await until(() =>
      records().find(
        r => r.type.startsWith('tool_') && r.id === id && !r.worker,
      ),
    );
    assert.equal(record.type, 'tool_result', record.error);
    return record.result;
  };
  const call = async (name, params) => (await run(name, params)).details;
  const outcomes = id =>
    records().filter(
      r =>
        r.type === 'owner_message' && !r.worker && r.message.details?.id === id,
    );
  // worker_wait returns the outcome it waited for; a returned outcome is never delivered again as an event.
  const waited = async (id, timeoutMs = 10000) => {
    const text = (await run('worker_wait', {id, timeoutMs})).content[0].text;
    return {kind: /^\S+ (?:is )?(\w+)/.exec(text)?.[1], text};
  };
  t.after(async () => {
    try {
      if (parent.alive) await call('worker_stop', {all: true});
    } finally {
      await parent.close();
      await http.close();
      assert.doesNotMatch(
        parent.stderr.join(''),
        /Failed to load extension|ERR_MODULE_NOT_FOUND/,
      );
      rmSync(dir, {recursive: true, force: true});
    }
  });
  await parent.state();
  return {parent, dir, http, records, run, call, outcomes, waited};
}

test(
  'real worker: same session retries, settles once, and can receive a follow-up',
  {timeout: 30000},
  async t => {
    const h = await setup(t, (_r, rs) => (rs.length === 1 ? 'stall' : 'ok'));
    const w = await h.call('worker_spawn', {
      label: 'recover',
      task: 'fixture work',
      model: 'inherit',
      thinking: 'inherit',
    });
    await until(() => h.http.requests.length === 1);
    await delay(180);
    assert.equal(
      h.outcomes(w.id).length,
      0,
      'no premature parent completion during backoff',
    );
    const during = await h.run('worker_status', {id: w.id});
    assert.match(during.content[0].text, /running|retrying/);
    const first = await h.waited(w.id);
    assert.equal(first.kind, 'completed');
    assert.match(first.text, /fixture ok/);
    const status = await h.call('worker_status', {id: w.id});
    assert.equal(status.state, 'completed');
    assert.equal(
      h
        .records()
        .filter(r => r.worker === w.id && r.type === 'guard_registered').length,
      1,
    );
    assert.equal(
      h.records().filter(r => r.worker === w.id && r.type === 'settled').length,
      1,
    );
    const session = status.session;
    await h.run('message', {to: w.id, text: 'second fixture task'});
    assert.equal((await h.waited(w.id)).kind, 'completed');
    assert.equal(
      h.outcomes(w.id).length,
      0,
      'waited outcomes are not delivered again',
    );
    assert.equal((await h.call('worker_status', {id: w.id})).session, session);
    assert.equal(h.http.requests.length, 3);
    assert.ok(
      h.http.requests.every(
        r => !JSON.stringify(r.payload).includes('retried automatically'),
      ),
    );
  },
);

test(
  'real worker: retry exhaustion is failed, not completed or running',
  {timeout: 30000},
  async t => {
    const h = await setup(t, () => 'stall');
    const w = await h.call('worker_spawn', {
      label: 'exhaust',
      task: 'fixture',
      model: 'inherit',
      thinking: 'inherit',
    });
    const outcome = await h.waited(w.id);
    assert.equal(outcome.kind, 'failed');
    assert.match(outcome.text, /timed out/);
    assert.equal((await h.call('worker_status', {id: w.id})).state, 'failed');
    assert.equal(h.http.requests.length, 2);
    await until(() => h.http.active.size === 0);
  },
);

test(
  'real workers: independent timers, stop closes stalled child without retry',
  {timeout: 30000},
  async t => {
    const h = await setup(t, r =>
      JSON.stringify(r.payload).includes('stall-lane') ? 'stall' : 'ok',
    );
    const a = await h.call('worker_spawn', {
      label: 'slow',
      task: 'stall-lane',
      model: 'inherit',
      thinking: 'inherit',
    });
    await until(() =>
      h.http.requests.some(r => r.headers['x-fixture-worker'] === a.id),
    );
    await h.call('worker_stop', {id: a.id});
    const b = await h.call('worker_spawn', {
      label: 'fast',
      task: 'healthy-lane',
      model: 'inherit',
      thinking: 'inherit',
    });
    assert.equal((await h.waited(b.id)).kind, 'completed');
    assert.equal((await h.call('worker_status', {id: a.id})).state, 'stopped');
    await delay(450);
    assert.equal(
      h.http.requests.filter(r => r.headers['x-fixture-worker'] === a.id)
        .length,
      1,
    );
    await until(() => h.http.active.size === 0);
  },
);

test(
  'real worker: parent shutdown cancels active child request',
  {timeout: 30000},
  async t => {
    const h = await setup(t, () => 'stall');
    await h.call('worker_spawn', {
      label: 'shutdown',
      task: 'fixture',
      model: 'inherit',
      thinking: 'inherit',
    });
    await until(() => h.http.requests.length === 1);
    await h.parent.close();
    await until(() => h.http.active.size === 0);
    await delay(450);
    assert.equal(h.http.requests.length, 1);
    assert.ok(h.records().some(r => r.worker && r.type === 'shutdown'));
  },
);

test(
  'real worker: messages queued during retry are delivered without duplicate completion',
  {timeout: 30000},
  async t => {
    const h = await setup(t, (_r, rs) => (rs.length === 1 ? 'stall' : 'ok'));
    const w = await h.call('worker_spawn', {
      label: 'steering',
      task: 'fixture',
      model: 'inherit',
      thinking: 'inherit',
    });
    await until(() => h.http.requests.length === 1);
    await delay(150);
    await h.run('message', {to: w.id, text: 'steering-marker'});
    assert.equal((await h.waited(w.id)).kind, 'completed');
    assert.equal(
      h.outcomes(w.id).length,
      0,
      'one completion, returned by the wait',
    );
    assert.ok(
      h.http.requests
        .slice(1)
        .some(r => JSON.stringify(r.payload).includes('steering-marker')),
    );
    assert.equal(
      h.records().filter(r => r.worker === w.id && r.type === 'settled').length,
      1,
    );
  },
);

test(
  'real worker: post-tool startup stall retries without executing tool twice',
  {timeout: 30000},
  async t => {
    const h = await setup(t, (_r, rs) =>
      rs.length === 1 ? 'tool' : rs.length === 2 ? 'stall' : 'ok',
    );
    const w = await h.call('worker_spawn', {
      label: 'post-tool',
      task: 'read fixture.txt',
      model: 'inherit',
      thinking: 'inherit',
    });
    assert.equal((await h.waited(w.id)).kind, 'completed');
    assert.equal(h.http.requests.length, 3);
    assert.ok(
      JSON.stringify(h.http.requests[1].payload).includes(
        'neutral fixture content',
      ),
    );
    const status = await h.call('worker_status', {id: w.id});
    assert.equal(status.calls, 1);
  },
);

test(
  'real nested workers: guard is inherited and nested retry settles only at its owner',
  {timeout: 30000},
  async t => {
    const lanes = new Map();
    const h = await setup(t, r => {
      const id = r.headers['x-fixture-worker'];
      const count = (lanes.get(id) ?? 0) + 1;
      lanes.set(id, count);
      if (count !== 1) return 'ok';
      return JSON.stringify(r.payload.messages).includes('nested-fixture')
        ? 'stall'
        : 'spawn';
    });
    const outer = await h.call('worker_spawn', {
      label: 'outer',
      task: 'fixture',
      model: 'inherit',
      thinking: 'inherit',
    });
    await h.call('worker_wait', {id: outer.id, timeoutMs: 10000});
    const record = await until(() =>
      h
        .records()
        .find(
          r =>
            r.worker && r.worker !== outer.id && r.type === 'guard_registered',
        ),
    );
    const nested = record.worker;
    const outcome = await until(() =>
      h
        .records()
        .find(
          r =>
            r.worker === outer.id &&
            r.type === 'owner_message' &&
            r.message.details?.id === nested,
        ),
    );
    assert.equal(outcome.message.details.kind, 'completed');
    assert.equal(
      h.http.requests.filter(r => r.headers['x-fixture-worker'] === nested)
        .length,
      2,
    );
    assert.equal(
      h
        .records()
        .filter(r => r.worker === nested && r.type === 'guard_registered')
        .length,
      1,
    );
    assert.equal(
      h.records().filter(r => r.worker === nested && r.type === 'settled')
        .length,
      1,
    );
    assert.equal(
      h.outcomes(nested).length,
      0,
      'nested completion belongs to its actual owner',
    );
    const observed = await h.run('worker_status', {id: nested});
    assert.match(
      observed.content[0].text,
      /auto-compaction off.*1\.0M capacity.*observed \d+s ago/,
    );
    assert.equal(observed.details.contextObservation.snapshot.compactAt, null);
    assert.equal(
      observed.details.contextObservation.snapshot.window,
      1_000_000,
    );
    assert.ok(observed.details.contextObservation.observedAt > 0);
    assert.ok(observed.details.contextObservation.snapshot.tokens > 0);
    await h.call('worker_stop', {id: outer.id});
    await until(() => h.http.active.size === 0);
  },
);

test(
  'real worker: parent question survives response recovery without being asked twice',
  {timeout: 30000},
  async t => {
    const h = await setup(t, (_r, rs) =>
      rs.length === 1 ? 'ask' : rs.length === 2 ? 'stall' : 'ok',
    );
    const w = await h.call('worker_spawn', {
      label: 'question',
      task: 'fixture',
      model: 'inherit',
      thinking: 'inherit',
    });
    await until(() =>
      h.outcomes(w.id).some(r => r.message.details.kind === 'asking'),
    );
    await h.run('message', {to: w.id, text: 'fixture-answer'});
    assert.equal((await h.waited(w.id)).kind, 'completed');
    assert.deepEqual(
      h.outcomes(w.id).map(r => r.message.details.kind),
      ['asking'],
      "asked once; the completion is the wait's result",
    );
    assert.equal(h.http.requests.length, 3);
    assert.ok(
      JSON.stringify(h.http.requests[2].payload).includes('fixture-answer'),
    );
  },
);

test(
  'real worker: manual compaction request is guarded without manufacturing a worker run',
  {timeout: 30000},
  async t => {
    const h = await setup(t, (_r, rs) => (rs.length === 3 ? 'stall' : 'ok'));
    const w = await h.call('worker_spawn', {
      label: 'compact',
      task: 'fixture',
      model: 'inherit',
      thinking: 'inherit',
    });
    await h.call('worker_wait', {id: w.id, timeoutMs: 10000});
    await h.run('message', {to: w.id, text: 'second fixture'});
    await h.call('worker_wait', {id: w.id, timeoutMs: 10000});
    // Commands are handled without starting a model run; message deliberately
    // reports that disposition instead of claiming a new worker run started.
    await assert.rejects(
      h.run('message', {to: w.id, text: '/fixture-compact'}),
      /handled the prompt without starting a run/,
    );
    await until(() =>
      h.records().some(r => r.worker === w.id && r.type === 'compacted'),
    );
    await until(() => h.http.active.size === 0);
    assert.ok(h.http.requests.length >= 4);
    const status = await h.call('worker_status', {id: w.id});
    const entries = readFileSync(status.session, 'utf8')
      .trim()
      .split('\n')
      .map(JSON.parse);
    assert.ok(
      entries.some(
        e =>
          e.type === 'custom' &&
          e.customType === 'anthropic-request-guard' &&
          e.data.counts.timeout === 1,
      ),
    );
    assert.equal(
      h.records().filter(r => r.worker === w.id && r.type === 'settled').length,
      2,
    );
  },
);

test(
  'real worker: native model override compacts above 300K without changing capacity',
  {timeout: 30000},
  async t => {
    const h = await setup(
      t,
      (record, requests) => {
        record.inputTokens = requests.length === 1 ? 310_000 : 1;
        return requests.length === 1 ? 'tool' : 'ok';
      },
      {
        modelOverrides: {'anthropic/claude-opus-5-5': {reserveTokens: 700_000}},
        keepRecentTokens: 0,
      },
    );
    const w = await h.call('worker_spawn', {
      label: 'native-compact',
      task: 'fixture',
      model: 'inherit',
      thinking: 'inherit',
    });
    await until(() =>
      h.records().some(r => r.worker === w.id && r.type === 'settled'),
    );
    assert.ok(
      h.records().some(r => r.worker === w.id && r.type === 'compacted'),
    );
    const status = await until(async () => {
      const snapshot = await h.call('worker_status', {id: w.id});
      return snapshot.state === 'running' ? undefined : snapshot;
    });
    assert.equal(status.state, 'completed');
    assert.equal(status.compactAt, 300_000);
    assert.equal(status.contextWindow, 1_000_000);
    assert.ok(status.contextTokens < 250_000);
    assert.equal(status.compactions, 1);
    const view = await h.run('worker_status', {id: w.id});
    assert.match(
      view.content[0].text,
      /compacts at 300k · 1\.0M capacity · compacted 1×/,
    );
    assert.ok(
      h.http.requests.length >= 3,
      'work, native summary, continuation',
    );
  },
);

test(
  'real concurrent workers: one lane exhausts while another finishes',
  {timeout: 30000},
  async t => {
    const h = await setup(t, r =>
      JSON.stringify(r.payload.messages).includes('stall-lane')
        ? 'stall'
        : 'slow-content',
    );
    const [a, b] = await Promise.all([
      h.call('worker_spawn', {
        label: 'stalled',
        task: 'stall-lane',
        model: 'inherit',
        thinking: 'inherit',
      }),
      h.call('worker_spawn', {
        label: 'healthy',
        task: 'healthy-lane',
        model: 'inherit',
        thinking: 'inherit',
      }),
    ]);
    const all = (await h.run('worker_wait', {all: true, timeoutMs: 10000}))
      .content[0].text;
    assert.match(all, new RegExp(`${a.id} stalled failed`));
    assert.match(all, new RegExp(`${b.id} healthy completed`));
    assert.equal(
      h.http.requests.filter(r => r.headers['x-fixture-worker'] === a.id)
        .length,
      2,
    );
    assert.equal(
      h.http.requests.filter(r => r.headers['x-fixture-worker'] === b.id)
        .length,
      1,
    );
  },
);
