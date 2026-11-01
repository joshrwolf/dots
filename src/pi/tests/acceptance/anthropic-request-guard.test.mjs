import assert from 'node:assert/strict';
import {mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import test from 'node:test';
import {guardProvider} from '../../extensions/anthropic-request-guard/guard.ts';
import guardExtension from '../../extensions/anthropic-request-guard/index.ts';
import {anthropicHttp} from '../fixtures/anthropic-http.mjs';

import {hostModule} from '../fixtures/host.ts';
const host = await import(hostModule('pi-coding-agent'));
const {anthropicProvider} = await import(
  hostModule('pi-ai', 'providers/anthropic.js')
);

async function setup(
  t,
  plan,
  {retries = 2, guard = true, overrides, backoffMs = 10} = {},
) {
  const dir = mkdtempSync(join(tmpdir(), 'dots-guard-host-'));
  const http = await anthropicHttp(plan);
  const saved = {...process.env};
  for (const key of Object.keys(process.env))
    if (key.startsWith('ANTHROPIC_')) delete process.env[key];
  process.env.ANTHROPIC_API_KEY = 'fixture';
  process.env.PI_CODING_AGENT_DIR = dir;
  const observations = [];
  const events = [];
  const errors = [];
  const stock = anthropicProvider();
  // Inject below the guard, retaining the real host's auth and request conversion.
  // Guard must wrap this fetch rather than replacing it.
  const provider = guard
    ? guardProvider(stock, {timeoutMs: 80, observe: o => observations.push(o)})
    : stock;
  const fixture = {
    api: 'anthropic-messages',
    streamSimple: (m, c, o) =>
      provider.streamSimple(m, c, {...o, fetch: http.fetch}),
  };
  const settings = host.SettingsManager.inMemory({
    compaction: {enabled: false, keepRecentTokens: 0},
    cacheWarming: 'off',
    retry: {
      enabled: true,
      maxRetries: retries,
      baseDelayMs: backoffMs,
      maxAgentDelayMs: backoffMs,
    },
  });
  const loader = new host.DefaultResourceLoader({
    cwd: dir,
    agentDir: dir,
    settingsManager: settings,
    noExtensions: true,
    noSkills: true,
    noPromptTemplates: true,
    noThemes: true,
    systemPromptOverride: () => 'Fixture.',
    extensionFactories: [pi => pi.registerProvider('anthropic', fixture)],
  });
  await loader.reload();
  if (overrides)
    writeFileSync(join(dir, 'overrides.json'), JSON.stringify(overrides));
  const runtime = await host.ModelRuntime.create({
    authPath: join(dir, 'auth.json'),
    modelsPath: overrides ? join(dir, 'overrides.json') : null,
    modelsStorePath: join(dir, 'models.json'),
    allowModelNetwork: false,
    refreshOnCreate: false,
  });
  const model = runtime.getModels().find(m => m.provider === 'anthropic');
  assert.ok(model);
  const {session} = await host.createAgentSession({
    cwd: dir,
    agentDir: dir,
    modelRuntime: runtime,
    model,
    settingsManager: settings,
    sessionManager: host.SessionManager.inMemory(dir),
    resourceLoader: loader,
    noTools: 'builtin',
  });
  await session.bindExtensions({mode: 'rpc', onError: e => errors.push(e)});
  session.subscribe(e => events.push(e));
  t.after(async () => {
    await session.abort();
    session.dispose();
    await http.close();
    for (const key of Object.keys(process.env))
      if (!(key in saved)) delete process.env[key];
    Object.assign(process.env, saved);
    rmSync(dir, {recursive: true, force: true});
    assert.deepEqual(errors, []);
  });
  return {session, http, observations, events};
}
async function until(predicate) {
  const end = Date.now() + 5000;
  while (!predicate()) {
    if (Date.now() > end) assert.fail('fixture deadline');
    await delay(5);
  }
}
const messages = h =>
  h.events
    .filter(e => e.type === 'message_end' && e.message.role === 'assistant')
    .map(e => e.message);

test(
  'installed host: ping-only startup retries natively and settles once',
  {timeout: 10000},
  async t => {
    const h = await setup(t, (_r, rs) => (rs.length === 1 ? 'stall' : 'ok'));
    await h.session.prompt('fixture');
    await until(() => h.http.active.size === 0);
    assert.equal(h.http.requests.length, 2);
    assert.deepEqual(
      messages(h).map(m => m.stopReason),
      ['error', 'stop'],
    );
    assert.equal(h.events.filter(e => e.type === 'auto_retry_start').length, 1);
    assert.equal(h.events.filter(e => e.type === 'agent_settled').length, 1);
    assert.equal(h.session.messages.filter(m => m.role === 'user').length, 1);
    assert.deepEqual(
      h.observations.map(o => o.outcome),
      ['timeout', 'started'],
    );
  },
);

test(
  'installed host: silent body exhausts configured retries and closes sockets',
  {timeout: 10000},
  async t => {
    const h = await setup(t, () => 'silent');
    await h.session.prompt('fixture');
    await until(() => h.http.active.size === 0);
    assert.equal(h.http.requests.length, 3);
    assert.deepEqual(
      messages(h).map(m => m.stopReason),
      ['error', 'error', 'error'],
    );
    assert.equal(h.events.filter(e => e.type === 'agent_settled').length, 1);
  },
);

test(
  'installed host: user cancellation does not retry',
  {timeout: 10000},
  async t => {
    const h = await setup(t, () => 'stall');
    const prompt = h.session.prompt('fixture');
    await until(() => h.http.requests.length === 1);
    await h.session.abort();
    await prompt;
    await until(() => h.http.active.size === 0);
    await delay(100);
    assert.equal(h.http.requests.length, 1);
    assert.equal(messages(h).at(-1).stopReason, 'aborted');
  },
);

for (const mode of ['ok', 'delayed-start', 'slow-content'])
  test(`installed host: ${mode} is unaffected`, {timeout: 10000}, async t => {
    const h = await setup(t, () => mode);
    await h.session.prompt('fixture');
    assert.equal(h.session.getLastAssistantText(), 'fixture ok');
    assert.equal(h.http.requests.length, 1);
    assert.equal(h.observations[0].outcome, 'started');
  });

test(
  'installed host: malformed SSE closes its reader without retry-loop leakage',
  {timeout: 10000},
  async t => {
    const h = await setup(t, () => 'malformed', {retries: 0});
    await h.session.prompt('fixture');
    await until(() => h.http.active.size === 0);
    assert.equal(messages(h).at(-1).stopReason, 'error');
    assert.equal(h.http.requests.length, 1);
  },
);

test(
  'installed host: compaction calls are guarded and retry independently',
  {timeout: 10000},
  async t => {
    const h = await setup(t, (_r, rs) => (rs.length === 3 ? 'stall' : 'ok'));
    await h.session.prompt('fixture one');
    await h.session.prompt('fixture two');
    const compacted = await h.session.compact();
    assert.match(compacted.summary, /fixture ok/);
    await until(() => h.http.active.size === 0);
    assert.equal(h.observations.filter(o => o.outcome === 'timeout').length, 1);
    assert.ok(h.http.requests.length >= 4);
  },
);

test(
  'installed host: zero retry budget yields one terminal failure',
  {timeout: 10000},
  async t => {
    const h = await setup(t, () => 'stall', {retries: 0});
    await h.session.prompt('fixture');
    assert.equal(h.http.requests.length, 1);
    assert.equal(messages(h).at(-1).stopReason, 'error');
    assert.equal(h.events.filter(e => e.type === 'auto_retry_start').length, 0);
  },
);

test(
  'installed host: cancellation during backoff prevents replacement request',
  {timeout: 10000},
  async t => {
    const h = await setup(t, () => 'stall', {backoffMs: 500});
    const prompt = h.session.prompt('fixture');
    await until(() => h.events.some(e => e.type === 'auto_retry_start'));
    await h.session.abort();
    await prompt;
    await delay(550);
    assert.equal(h.http.requests.length, 1);
    await until(() => h.http.active.size === 0);
  },
);

test(
  'installed host: models.json endpoint and headers survive streaming override',
  {timeout: 10000},
  async t => {
    const h = await setup(t, () => 'stall', {
      overrides: {
        providers: {
          anthropic: {
            baseUrl: 'https://gateway.example.invalid',
            headers: {'x-fixture-override': 'retained'},
          },
        },
      },
    });
    const prompt = h.session.prompt('fixture');
    await until(() => h.http.requests.length === 1);
    await delay(150);
    assert.equal(h.http.requests[0].headers['x-fixture-override'], 'retained');
    assert.equal(h.observations.length, 0, 'custom endpoint is not guarded');
    await h.session.abort();
    await prompt;
  },
);

test('installed provider: federation token cache survives sequential guarded requests', async t => {
  const dir = mkdtempSync(join(tmpdir(), 'dots-guard-federation-'));
  t.after(() => rmSync(dir, {recursive: true, force: true}));
  const tokenFile = join(dir, 'identity-token');
  writeFileSync(tokenFile, 'fixture-identity');
  const env = {
    ANTHROPIC_API_KEY: '',
    ANTHROPIC_OAUTH_TOKEN: '',
    ANTHROPIC_FEDERATION_RULE_ID: 'fixture-rule',
    ANTHROPIC_ORGANIZATION_ID: 'fixture-org',
    ANTHROPIC_IDENTITY_TOKEN_FILE: tokenFile,
  };
  const frames = [
    {
      type: 'message_start',
      message: {
        id: 'fixture',
        type: 'message',
        role: 'assistant',
        model: 'fixture',
        content: [],
        usage: {input_tokens: 1, output_tokens: 0},
      },
    },
    {
      type: 'message_delta',
      delta: {stop_reason: 'end_turn'},
      usage: {output_tokens: 0},
    },
    {type: 'message_stop'},
  ]
    .map(event => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`)
    .join('');
  for (const guarded of [false, true]) {
    const paths = [];
    const fetch = async input => {
      const path = new URL(input instanceof Request ? input.url : String(input))
        .pathname;
      paths.push(path);
      if (path === '/v1/oauth/token')
        return Response.json({
          access_token: 'fixture-access',
          token_type: 'Bearer',
          expires_in: 3600,
        });
      assert.equal(path, '/v1/messages');
      return new Response(frames, {
        headers: {'content-type': 'text/event-stream'},
      });
    };
    const stock = anthropicProvider();
    const provider = guarded ? guardProvider(stock) : stock;
    for (let i = 0; i < 3; i++) {
      const result = await provider
        .streamSimple(
          stock.getModels()[0],
          {messages: [{role: 'user', content: 'fixture', timestamp: 0}]},
          {env, fetch, maxTokens: 8},
        )
        .result();
      assert.equal(result.stopReason, 'stop', result.errorMessage);
    }
    assert.deepEqual(
      paths,
      ['/v1/oauth/token', '/v1/messages', '/v1/messages', '/v1/messages'],
      `guarded=${guarded}`,
    );
  }
});

test('shipping registration preserves remote catalog, refresh and removable config overlays', async t => {
  const dir = mkdtempSync(join(tmpdir(), 'dots-guard-composition-'));
  t.after(() => rmSync(dir, {recursive: true, force: true}));
  const path = join(dir, 'models.json');
  const remote = {
    ...anthropicProvider().getModels()[0],
    id: 'fixture-remote-model',
    name: 'Remote fixture',
  };
  const store = new Map([
    ['anthropic', {models: [remote], lastModified: 8_000_000_000_000}],
  ]);
  writeFileSync(
    path,
    JSON.stringify({
      providers: {
        anthropic: {
          headers: {'x-fixture-config': 'retained'},
          modelOverrides: {[remote.id]: {name: 'Configured fixture'}},
        },
      },
    }),
  );
  const runtime = await host.ModelRuntime.create({
    authPath: join(dir, 'auth.json'),
    modelsPath: path,
    modelsStore: {
      read: async id => store.get(id),
      write: async (id, entry) => store.set(id, entry),
      delete: async id => store.delete(id),
    },
    allowModelNetwork: false,
  });
  const before = runtime.getModels('anthropic');
  const install = () =>
    guardExtension(
      {
        registerProvider: (...args) => runtime.registerProvider(...args),
        on() {},
        registerCommand() {},
      },
      {timeoutMs: 30},
    );
  install();
  assert.equal(runtime.getRegisteredNativeProvider('anthropic'), undefined);
  assert.deepEqual(runtime.getModels('anthropic'), before);
  assert.equal(
    typeof runtime.getProvider('anthropic').refreshModels,
    'function',
  );
  runtime.registerProvider('anthropic', {
    headers: {'x-fixture-peer': 'preserved'},
  });
  const context = {
    messages: [{role: 'user', content: 'fixture', timestamp: 0}],
  };
  const capture = async options => {
    let headers;
    const result = await runtime.completeSimple(
      runtime.getModel('anthropic', remote.id),
      context,
      {
        apiKey: 'fixture',
        ...options,
        fetch: async (_input, init) => {
          headers = new Headers(init.headers);
          return new Response('', {
            headers: {'content-type': 'text/event-stream'},
          });
        },
      },
    );
    assert.notEqual(result.stopReason, 'aborted');
    return headers;
  };
  let headers = await capture();
  assert.equal(headers.get('x-fixture-config'), 'retained');
  assert.equal(headers.get('x-fixture-peer'), 'preserved');
  writeFileSync(path, '{}');
  await runtime.refresh({allowNetwork: false});
  install(); // Reload replaces the handler rather than wrapping a composed provider.
  headers = await capture();
  assert.equal(headers.get('x-fixture-config'), null);
  assert.equal(headers.get('x-fixture-peer'), 'preserved');
  assert.equal(runtime.getModel('anthropic', remote.id).name, 'Remote fixture');
  const result = await runtime.completeSimple(
    runtime.getModel('anthropic', remote.id),
    context,
    {
      apiKey: 'fixture',
      fetch: async () =>
        new Response(new ReadableStream({}), {
          headers: {'content-type': 'text/event-stream'},
        }),
    },
  );
  assert.match(result.errorMessage, /timed out.*message_start/);
});

test('shipping override explicitly uses simple reasoning semantics for full API calls', async t => {
  const dir = mkdtempSync(join(tmpdir(), 'dots-guard-options-'));
  t.after(() => rmSync(dir, {recursive: true, force: true}));
  const runtime = await host.ModelRuntime.create({
    authPath: join(dir, 'auth.json'),
    modelsPath: null,
    modelsStorePath: join(dir, 'store.json'),
    refreshOnCreate: false,
  });
  const model = runtime.getModel('anthropic', 'claude-haiku-4-5');
  assert.ok(model);
  const context = {
    messages: [{role: 'user', content: 'fixture', timestamp: 0}],
  };
  const payload = async options => {
    let body;
    await runtime.complete(model, context, {
      apiKey: 'fixture',
      ...options,
      fetch: async (_input, init) => {
        body = JSON.parse(init.body);
        return new Response('', {
          headers: {'content-type': 'text/event-stream'},
        });
      },
    });
    return body;
  };
  const advanced = {
    thinkingEnabled: true,
    thinkingBudgetTokens: 2048,
    maxTokens: 4096,
  };
  const original = (await payload(advanced)).thinking;
  assert.equal(original.type, 'enabled');
  assert.equal(original.budget_tokens, 2048);
  guardExtension({
    registerProvider: (...args) => runtime.registerProvider(...args),
    on() {},
    registerCommand() {},
  });
  assert.deepEqual((await payload(advanced)).thinking, {type: 'disabled'});
  assert.equal(
    (await payload({reasoning: 'high', maxTokens: 4096})).thinking.type,
    'enabled',
  );
});

const baselineMs = Number(process.env.PI_GUARD_BASELINE_MS ?? 200);
if (!Number.isSafeInteger(baselineMs) || baselineMs < 1 || baselineMs > 120000)
  throw new Error('PI_GUARD_BASELINE_MS must be 1–120000');
test(
  'stock baseline: report upstream startup-stall behavior without requiring it to stay broken',
  {timeout: baselineMs + 10000},
  async t => {
    const h = await setup(t, () => 'stall', {guard: false, retries: 0});
    let settled = false;
    const prompt = h.session.prompt('fixture').then(() => {
      settled = true;
    });
    await until(() => h.http.requests.length === 1);
    await delay(baselineMs);
    t.diagnostic(
      JSON.stringify({
        baseline: 'stock',
        windowMs: baselineMs,
        settled,
        stopReason: messages(h).at(-1)?.stopReason,
        interpretation: settled
          ? 'Re-evaluate upstream behavior and guard necessity'
          : 'No startup bound observed in this window; not proof of an infinite stall',
      }),
    );
    await h.session.abort();
    await prompt;
  },
);
