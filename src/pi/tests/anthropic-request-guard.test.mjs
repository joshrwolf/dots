import assert from 'node:assert/strict';
import test from 'node:test';
import {setTimeout as delay} from 'node:timers/promises';
import {anthropicProvider} from '@earendil-works/pi-ai/providers/anthropic';
import {guardProvider} from '../extensions/anthropic-request-guard/guard.ts';
import extension from '../extensions/anthropic-request-guard/index.ts';
import {
  Diagnostics,
  ENTRY_TYPE,
  MAX_ENTRIES,
  MAX_RECENT,
} from '../extensions/anthropic-request-guard/diagnostics.ts';

const encode = (type, fields = {}) =>
  new TextEncoder().encode(
    `event: ${type}\ndata: ${JSON.stringify({type, ...fields})}\n\n`,
  );
const start = encode('message_start', {
  message: {
    id: 'fixture',
    type: 'message',
    role: 'assistant',
    model: 'fixture',
    content: [],
    usage: {input_tokens: 1, output_tokens: 0},
  },
});
const end = [
  encode('message_delta', {
    delta: {stop_reason: 'end_turn'},
    usage: {output_tokens: 1},
  }),
  encode('message_stop'),
];
const context = {messages: [{role: 'user', content: 'fixture', timestamp: 0}]};

function fixture(t, frames = []) {
  let body;
  let cancelled = 0;
  let signal;
  const fetch = async (_url, init) => {
    signal = init.signal;
    return new Response(
      new ReadableStream({
        start(c) {
          body = c;
          for (const frame of frames) c.enqueue(frame);
        },
        cancel() {
          cancelled++;
        },
      }),
      {headers: {'content-type': 'text/event-stream'}},
    );
  };
  t.after(() => {
    try {
      body?.close();
    } catch {}
  });
  return {
    fetch,
    push: f => body.enqueue(f),
    close: () => body.close(),
    get cancelled() {
      return cancelled;
    },
    get signal() {
      return signal;
    },
  };
}
const stock = anthropicProvider();
const model = stock.getModels()[0];
function request(provider, fetch, options = {}) {
  return provider
    .streamSimple(model, context, {
      apiKey: 'fixture',
      fetch,
      maxTokens: 8,
      ...options,
    })
    .result();
}

test('diagnostics retain only bounded metadata and snapshots are independent', () => {
  const d = new Diagnostics('fixture-version');
  for (let i = 0; i < 100; i++)
    d.record({
      model: 'fixture',
      outcome: 'started',
      elapsedMs: i,
      payload: 'must not persist',
    });
  const snapshot = d.snapshot();
  assert.equal(snapshot.recent.length, MAX_RECENT);
  assert.equal(snapshot.counts.started, 100);
  assert.equal(snapshot.maxStartMs, 99);
  assert.equal(snapshot.meanStartMs, 50);
  assert.ok(!JSON.stringify(snapshot).includes('must not persist'));
  snapshot.counts.started = 0;
  snapshot.recent[0].model = 'changed';
  assert.equal(d.snapshot().counts.started, 100);
  assert.equal(d.snapshot().recent[0].model, 'fixture');
});

test('extension caps session entries across reload and never injects a message', async t => {
  const entries = Array.from({length: MAX_ENTRIES - 1}, () => ({
    type: 'custom',
    customType: ENTRY_TYPE,
  }));
  for (let n = 0; n < 2; n++) {
    const handlers = {};
    const commands = {};
    let provider;
    extension({
      registerProvider: (name, p) => {
        assert.equal(name, 'anthropic');
        assert.equal(p.api, 'anthropic-messages');
        provider = p;
      },
      on: (name, fn) => {
        handlers[name] = fn;
      },
      registerCommand: (name, command) => {
        commands[name] = command;
      },
      appendEntry: (customType, data) =>
        entries.push({type: 'custom', customType, data}),
      sendMessage: () =>
        assert.fail('guard must not inject conversation messages'),
    });
    handlers.session_start({}, {sessionManager: {getEntries: () => entries}});
    const f = fixture(t, [start, ...end]);
    const result = request(provider, f.fetch);
    while (!f.signal) await delay(1);
    f.close();
    await result;
    handlers.session_shutdown();
    assert.equal(entries.length, MAX_ENTRIES);
    assert.ok(commands['request-guard']);
  }
});

test('ping-only body times out and cancels without aborting caller', async t => {
  const f = fixture(t, [encode('ping')]);
  const observations = [];
  const caller = new AbortController();
  const provider = guardProvider(stock, {
    timeoutMs: 20,
    observe: o => observations.push(o),
  });
  const result = await request(provider, f.fetch, {signal: caller.signal});
  assert.equal(result.stopReason, 'error');
  assert.match(result.errorMessage, /timed out.*message_start/);
  assert.equal(caller.signal.aborted, false);
  assert.equal(f.cancelled, 1);
  assert.equal(f.signal.aborted, true);
  assert.deepEqual(
    observations.map(o => o.outcome),
    ['timeout'],
  );
});

test('actual message_start disables timer while content remains slow', async t => {
  const f = fixture(t, [start]);
  const observations = [];
  let observedStart;
  const began = new Promise(resolve => {
    observedStart = resolve;
  });
  const provider = guardProvider(stock, {
    timeoutMs: 20,
    observe: o => {
      observations.push(o);
      observedStart();
    },
  });
  const result = request(provider, f.fetch);
  await began;
  await delay(50);
  for (const frame of end) f.push(frame);
  f.close();
  assert.equal((await result).stopReason, 'stop');
  assert.equal(f.cancelled, 0);
  assert.deepEqual(
    observations.map(o => o.outcome),
    ['started'],
  );
});

test('caller cancellation is aborted rather than a retryable timeout', async t => {
  const f = fixture(t);
  const caller = new AbortController();
  const result = request(guardProvider(stock, {timeoutMs: 1000}), f.fetch, {
    signal: caller.signal,
  });
  while (!f.signal) await delay(1);
  caller.abort();
  assert.equal((await result).stopReason, 'aborted');
  assert.equal(f.cancelled, 1);
});

test('parse failure cancels open body even after message_start removed timer', async t => {
  const f = fixture(t, [
    start,
    new TextEncoder().encode(
      'event: content_block_delta\ndata: definitely-not-json\n\n',
    ),
  ]);
  const result = await request(
    guardProvider(stock, {timeoutMs: 1000}),
    f.fetch,
  );
  assert.equal(result.stopReason, 'error');
  await delay(0);
  assert.equal(f.cancelled, 1);
  assert.equal(f.signal.aborted, true);
});

test('decorator preserves supplied provider methods; custom endpoints bypass guard', async t => {
  const f = fixture(t, [start, ...end]);
  const observations = [];
  const p = guardProvider(stock, {
    timeoutMs: 10,
    observe: o => observations.push(o),
  });
  assert.equal(p.auth, stock.auth);
  assert.equal(p.getModels, stock.getModels);
  const result = p
    .streamSimple(
      {...model, baseUrl: 'https://gateway.example.invalid'},
      context,
      {apiKey: 'fixture', fetch: f.fetch},
    )
    .result();
  while (!f.signal) await delay(1);
  f.close();
  assert.equal((await result).stopReason, 'stop');
  assert.deepEqual(observations, []);
});

test('callbacks and diagnostic failure cannot break response', async t => {
  const f = fixture(t, [start, ...end]);
  const seen = [];
  const p = guardProvider(stock, {
    timeoutMs: 10,
    observe: () => {
      throw new Error('diagnostic unavailable');
    },
  });
  const result = request(p, f.fetch, {
    onPayload: payload => {
      seen.push('payload');
      return payload;
    },
    onResponse: () => seen.push('headers'),
    onProviderStreamEvent: event => seen.push(event.type),
  });
  while (!seen.includes('message_stop')) await delay(1);
  f.close();
  assert.equal((await result).stopReason, 'stop');
  assert.deepEqual(seen, [
    'payload',
    'headers',
    'message_start',
    'message_delta',
    'message_stop',
  ]);
});

test('full stream entry point uses the startup guard', async t => {
  const f = fixture(t, [encode('ping')]);
  const p = guardProvider(stock, {timeoutMs: 20});
  const result = await p
    .stream(model, context, {apiKey: 'fixture', fetch: f.fetch})
    .result();
  assert.equal(result.stopReason, 'error');
  assert.match(result.errorMessage, /timed out/);
});

test('early EOF and non-streaming HTTP errors retain stock behavior', async () => {
  const observations = [];
  const p = guardProvider(stock, {
    timeoutMs: 10,
    observe: o => observations.push(o),
  });
  const httpError = await request(
    p,
    async () =>
      new Response(
        '{"type":"error","error":{"type":"invalid_request_error","message":"fixture rejected"}}',
        {status: 400, headers: {'content-type': 'application/json'}},
      ),
  );
  assert.equal(httpError.stopReason, 'error');
  assert.match(httpError.errorMessage, /fixture rejected/);
  assert.equal(observations.length, 0);
  await request(
    p,
    async () =>
      new Response('', {headers: {'content-type': 'text/event-stream'}}),
  );
  assert.deepEqual(
    observations.map(o => o.outcome),
    ['ended'],
  );
});

test('pre-header abort preserves caller semantics without arming deadline', async () => {
  const controller = new AbortController();
  const observations = [];
  const p = guardProvider(stock, {
    timeoutMs: 10,
    observe: o => observations.push(o),
  });
  let sent;
  const begun = new Promise(resolve => {
    sent = resolve;
  });
  const result = request(
    p,
    async (_url, init) => {
      sent();
      return new Promise((_resolve, reject) => {
        if (init.signal.aborted) reject(init.signal.reason);
        else
          init.signal.addEventListener(
            'abort',
            () => reject(init.signal.reason),
            {once: true},
          );
      });
    },
    {signal: controller.signal},
  );
  await begun;
  await delay(25);
  controller.abort();
  assert.equal((await result).stopReason, 'aborted');
  assert.deepEqual(observations, []);
});

test('concurrent requests keep independent startup timers', async t => {
  const stalled = fixture(t, [encode('ping')]);
  const healthy = fixture(t, [start, ...end]);
  const observations = [];
  const p = guardProvider(stock, {
    timeoutMs: 20,
    observe: o => observations.push(o),
  });
  let calls = 0;
  const fetch = (...args) =>
    (++calls === 1 ? stalled.fetch : healthy.fetch)(...args);
  const a = request(p, fetch);
  const b = request(p, fetch);
  while (!healthy.signal) await delay(1);
  healthy.close();
  assert.equal((await b).stopReason, 'stop');
  assert.equal((await a).stopReason, 'error');
  assert.deepEqual(observations.map(o => o.outcome).sort(), [
    'started',
    'timeout',
  ]);
});
