import assert from 'node:assert/strict';
import test from 'node:test';
import {setTimeout as delay} from 'node:timers/promises';
import {isRetryableAssistantError} from '@earendil-works/pi-ai';
import {anthropicProvider} from '@earendil-works/pi-ai/providers/anthropic';
import {guardStreamSimple} from '../extensions/anthropic-request-guard/guard.ts';
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
const malformed = new TextEncoder().encode(
  'event: content_block_delta\ndata: definitely-not-json\n\n',
);
const context = {messages: [{role: 'user', content: 'fixture', timestamp: 0}]};

/**
 * A fetch that honors its signal like Node's: an abort errors the open body,
 * after `abortDelayMs` to model a slow transport. No network.
 */
function fixture(t, frames = [], {abortDelayMs = 0} = {}) {
  let body;
  let signal;
  let requests = 0;
  const fetch = async (_url, init) => {
    requests++;
    signal = init.signal;
    signal.throwIfAborted();
    return new Response(
      new ReadableStream({
        start(c) {
          body = c;
          for (const frame of frames) c.enqueue(frame);
          signal.addEventListener(
            'abort',
            () =>
              setTimeout(() => {
                try {
                  c.error(signal.reason);
                } catch {}
              }, abortDelayMs),
            {once: true},
          );
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
    get requests() {
      return requests;
    },
    get signal() {
      return signal;
    },
  };
}
const stock = anthropicProvider();
const model = stock
  .getModels()
  .find(m => m.baseUrl === 'https://api.anthropic.com');
function guarded(timeoutMs, observations = []) {
  return guardStreamSimple(stock, {
    timeoutMs,
    observe: o => observations.push(o),
  });
}
function request(stream, fetch, options = {}, target = model) {
  return stream(target, context, {
    apiKey: 'fixture',
    fetch,
    maxTokens: 8,
    ...options,
  }).result();
}
const outcomes = observations => observations.map(o => o.outcome);

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
    const result = request(provider.streamSimple, f.fetch);
    while (!f.signal) await delay(1);
    f.close();
    await result;
    handlers.session_shutdown();
    assert.equal(entries.length, MAX_ENTRIES);
    assert.ok(commands['request-guard']);
  }
});

test('ping-only body becomes one retryable timeout and closes only its own transport', async t => {
  const f = fixture(t, [encode('ping')]);
  const observations = [];
  const caller = new AbortController();
  const result = await request(guarded(20, observations), f.fetch, {
    signal: caller.signal,
  });
  assert.equal(result.stopReason, 'error');
  assert.match(result.errorMessage, /timed out.*message_start/);
  assert.ok(isRetryableAssistantError(result));
  assert.equal(caller.signal.aborted, false);
  assert.equal(f.signal.aborted, true);
  assert.deepEqual(outcomes(observations), ['timeout']);
});

test('actual message_start disarms the deadline while content stays slow', async t => {
  const f = fixture(t, [start]);
  const observations = [];
  const result = request(guarded(20, observations), f.fetch);
  while (observations.length === 0) await delay(1);
  await delay(60);
  assert.equal(f.signal.aborted, false, 'healthy stream left open');
  for (const frame of end) f.push(frame);
  f.close();
  assert.equal((await result).stopReason, 'stop');
  assert.deepEqual(outcomes(observations), ['started']);
  assert.equal(f.signal.aborted, true, 'completion releases the transport');
});

for (const phase of ['after headers', 'after message_start'])
  test(`caller cancellation ${phase} stays aborted, never a timeout`, async t => {
    const f = fixture(t, phase === 'after headers' ? [] : [start]);
    const observations = [];
    const caller = new AbortController();
    const result = request(guarded(1000, observations), f.fetch, {
      signal: caller.signal,
      onResponse: () => {
        if (phase === 'after headers') setTimeout(() => caller.abort(), 5);
      },
      onProviderStreamEvent: e => {
        if (e.type === 'message_start') setTimeout(() => caller.abort(), 5);
      },
    });
    const message = await result;
    assert.equal(message.stopReason, 'aborted');
    assert.doesNotMatch(message.errorMessage, /timed out/);
    assert.equal(f.signal.aborted, true);
    assert.deepEqual(
      outcomes(observations),
      phase === 'after headers' ? ['cancelled'] : ['started'],
    );
  });

test('caller abort landing after the deadline fired still ends aborted', async t => {
  const f = fixture(t, [encode('ping')], {abortDelayMs: 40});
  const observations = [];
  const caller = new AbortController();
  const result = request(guarded(20, observations), f.fetch, {
    signal: caller.signal,
  });
  while (!f.signal?.aborted) await delay(1);
  caller.abort();
  assert.equal((await result).stopReason, 'aborted');
  assert.deepEqual(outcomes(observations), ['cancelled']);
});

test('pre-header abort keeps caller semantics without arming the deadline', async () => {
  const caller = new AbortController();
  const observations = [];
  let sent;
  const begun = new Promise(resolve => {
    sent = resolve;
  });
  const result = request(
    guarded(10, observations),
    async (_url, init) => {
      sent();
      return new Promise((_resolve, reject) => {
        init.signal.addEventListener(
          'abort',
          () => reject(init.signal.reason),
          {
            once: true,
          },
        );
      });
    },
    {signal: caller.signal},
  );
  await begun;
  await delay(25);
  caller.abort();
  assert.equal((await result).stopReason, 'aborted');
  assert.deepEqual(observations, []);
});

test('malformed SSE after message_start errors without a timeout and closes the body', async t => {
  const f = fixture(t, [start, malformed]);
  const observations = [];
  const result = await request(guarded(1000, observations), f.fetch);
  assert.equal(result.stopReason, 'error');
  assert.match(result.errorMessage, /Could not parse Anthropic SSE/);
  assert.equal(f.signal.aborted, true);
  assert.deepEqual(outcomes(observations), ['started']);
});

test('throwing caller onResponse surfaces its error, disarms and closes', async t => {
  const f = fixture(t, [encode('ping')]);
  const observations = [];
  const result = await request(guarded(20, observations), f.fetch, {
    onResponse: () => {
      throw new Error('fixture hook failure');
    },
  });
  assert.equal(result.stopReason, 'error');
  assert.match(result.errorMessage, /fixture hook failure/);
  assert.equal(f.signal.aborted, true);
  await delay(40);
  assert.deepEqual(outcomes(observations), ['ended']);
});

test('synchronous setup failure propagates like stock', () => {
  // Providers may throw before returning a stream, such as on missing auth.
  const failing = {
    ...stock,
    streamSimple() {
      throw new Error('No API key for provider: anthropic');
    },
  };
  assert.throws(
    () => guardStreamSimple(failing, {timeoutMs: 20})(model, context, {}),
    /No API key/,
  );
});

test('custom endpoints bypass the guard and keep caller options as sent', async t => {
  const f = fixture(t, [encode('ping')]);
  const observations = [];
  const caller = new AbortController();
  let settled = false;
  const result = request(
    guarded(10, observations),
    f.fetch,
    {signal: caller.signal},
    {...model, baseUrl: 'https://gateway.example.invalid'},
  ).then(m => {
    settled = true;
    return m;
  });
  await delay(60);
  assert.equal(settled, false, 'no deadline on a custom endpoint');
  caller.abort();
  assert.equal((await result).stopReason, 'aborted');
  assert.deepEqual(observations, []);
});

test('caller hooks chain in order and diagnostic failure cannot break a response', async t => {
  const f = fixture(t, [start, ...end]);
  const seen = [];
  const stream = guardStreamSimple(stock, {
    timeoutMs: 10,
    observe: () => {
      throw new Error('diagnostic unavailable');
    },
  });
  const result = request(stream, f.fetch, {
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

test('early EOF and non-streaming HTTP errors retain stock behavior', async () => {
  const observations = [];
  const stream = guarded(10, observations);
  const httpError = await request(
    stream,
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
    stream,
    async () =>
      new Response('', {headers: {'content-type': 'text/event-stream'}}),
  );
  assert.deepEqual(outcomes(observations), ['ended']);
});

test('concurrent requests keep independent deadlines', async t => {
  const stalled = fixture(t, [encode('ping')]);
  const healthy = fixture(t, [start, ...end]);
  const observations = [];
  const stream = guarded(20, observations);
  let calls = 0;
  const fetch = (...args) =>
    (++calls === 1 ? stalled.fetch : healthy.fetch)(...args);
  const a = request(stream, fetch);
  const b = request(stream, fetch);
  while (!healthy.signal) await delay(1);
  healthy.close();
  assert.equal((await b).stopReason, 'stop');
  assert.equal((await a).stopReason, 'error');
  assert.deepEqual(outcomes(observations).sort(), ['started', 'timeout']);
});
