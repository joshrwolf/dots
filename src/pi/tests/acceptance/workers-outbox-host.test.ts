// Provider-free boundary acceptance against the pinned Pi host, not a mock runner.
import assert from 'node:assert/strict';
import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {setTimeout as sleep} from 'node:timers/promises';
import test from 'node:test';
import {Type} from 'typebox';
import type {AssistantMessage, TranscriptContext} from '@earendil-works/pi-ai';
import type {
  CustomMessageEntry,
  ExtensionAPI,
  SessionEntry,
} from '@earendil-works/pi-coding-agent';
import {WorkerDelivery} from '../../extensions/workers/outbox.ts';
import {hostModule} from '../fixtures/host.ts';

const host: typeof import('@earendil-works/pi-coding-agent') = await import(
  hostModule('pi-coding-agent')
);
const ai: typeof import('@earendil-works/pi-ai') = await import(
  hostModule('pi-ai')
);
const item = (id: string) => ({
  key: `${id}:1`,
  content: `Worker ${id} [${id}] completed\n\nFinal ${id}`,
  details: {
    id,
    label: id,
    kind: 'completed',
    tokens: 9,
    context: 31,
    cost: 0.004,
  },
});
async function until(predicate: () => boolean) {
  for (let n = 0; n < 1000; n++) {
    if (predicate()) return;
    await sleep(10);
  }
  throw new Error('worker outbox host deadline');
}
function gate() {
  const {promise, resolve} = Promise.withResolvers<void>();
  return {promise, release: () => resolve()};
}
async function setup() {
  const dir = mkdtempSync(join(tmpdir(), 'dots-workers-outbox-host-'));
  const requests: TranscriptContext[] = [];
  const errors: unknown[] = [];
  const wait = gate();
  let pi!: ExtensionAPI;
  let delivery!: WorkerDelivery;
  const settings = host.SettingsManager.inMemory({
    compaction: {enabled: false},
    retry: {enabled: false},
    cacheWarming: 'off',
  });
  const loader = new host.DefaultResourceLoader({
    cwd: dir,
    agentDir: dir,
    settingsManager: settings,
    noExtensions: true,
    noSkills: true,
    noPromptTemplates: true,
    noThemes: true,
    extensionFactories: [
      (api: ExtensionAPI) => {
        pi = api;
        delivery = new WorkerDelivery(
          api,
          () => false,
          () => true,
        );
        api.on('session_shutdown', () => delivery.close());
        api.registerTool({
          name: 'fixture_wait',
          label: 'Fixture wait',
          description: 'Wait for a test gate',
          parameters: Type.Object({}),
          async execute() {
            await wait.promise;
            return {
              content: [{type: 'text', text: 'gate released'}],
              details: undefined,
            };
          },
        });
      },
    ],
  });
  await loader.reload();
  const runtime = await host.ModelRuntime.create({
    authPath: join(dir, 'auth.json'),
    modelsPath: null,
    modelsStorePath: join(dir, 'models-store.json'),
    allowModelNetwork: false,
    refreshOnCreate: false,
  });
  runtime.registerProvider('worker-outbox-fixture', {
    api: 'worker-outbox-fixture',
    apiKey: 'fixture',
    baseUrl: 'https://example.invalid',
    models: [
      {
        id: 'fixture',
        name: 'Fixture',
        reasoning: false,
        input: ['text'],
        contextWindow: 100_000,
        maxTokens: 1000,
        cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0},
      },
    ],
    streamSimple(model, context) {
      const stream = ai.createAssistantMessageEventStream();
      requests.push(structuredClone(context));
      const blocked =
        JSON.stringify(context.messages).includes('blocked request') &&
        !context.messages.some(
          message =>
            message.role === 'toolResult' &&
            message.toolName === 'fixture_wait',
        );
      const reason = blocked ? ('toolUse' as const) : ('stop' as const);
      const content: AssistantMessage['content'] = blocked
        ? [
            {
              type: 'toolCall',
              id: 'fixture-call',
              name: 'fixture_wait',
              arguments: {},
            },
          ]
        : [{type: 'text', text: 'owner acknowledged results'}];
      const message: AssistantMessage = {
        role: 'assistant',
        content,
        api: model.api,
        provider: model.provider,
        model: model.id,
        timestamp: Date.now(),
        stopReason: reason,
        usage: {
          input: 1,
          output: 1,
          cacheRead: 0,
          cacheWrite: 0,
          totalTokens: 2,
          cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0},
        },
      };
      queueMicrotask(() => {
        stream.push({type: 'start', partial: message});
        stream.push({type: 'done', reason, message});
        stream.end();
      });
      return stream;
    },
  });
  const model = runtime.getModel('worker-outbox-fixture', 'fixture');
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
  await session.bindExtensions({
    mode: 'rpc',
    onError: error => errors.push(error),
  });
  return {
    session,
    delivery,
    requests,
    wait,
    async close() {
      wait.release();
      await session.abort();
      await session.extensionRunner.emit({
        type: 'session_shutdown',
        reason: 'quit',
      });
      session.dispose();
      assert.deepEqual(errors, []);
      rmSync(dir, {recursive: true, force: true});
    },
  };
}
type WorkerEvent = CustomMessageEntry<{deliveryIds?: string[]}>;
const workerEvent = (entry: SessionEntry): entry is WorkerEvent =>
  entry.type === 'custom_message' && entry.customType === 'dots.worker.event';
const messages = (session: Awaited<ReturnType<typeof setup>>['session']) =>
  session.sessionManager.getBranch().filter(workerEvent);

test(
  'pinned host coalesces idle completions into one turn and busy completions at one boundary',
  {timeout: 20_000},
  async () => {
    const h = await setup();
    try {
      h.delivery.outbox.offer(item('w-one'));
      h.delivery.outbox.offer(item('w-two'));
      await until(
        () =>
          h.requests.length === 1 &&
          !h.session.isStreaming &&
          !h.delivery.outbox.hasPending(),
      );
      assert.equal(messages(h.session).length, 1);
      assert.deepEqual(messages(h.session)[0]?.details?.deliveryIds, [
        'w-one:1',
        'w-two:1',
      ]);
      assert.match(
        JSON.stringify(h.requests[0]?.messages),
        /Final w-one[\s\S]*Final w-two/,
      );

      const blocked = h.session.prompt('blocked request');
      await until(() => h.requests.length === 2);
      h.delivery.outbox.offer(item('w-three'));
      h.delivery.outbox.offer(item('w-four'));
      h.wait.release();
      await blocked;
      await until(
        () =>
          h.requests.length === 3 &&
          !h.session.isStreaming &&
          !h.delivery.outbox.hasPending(),
      );
      await sleep(150);
      assert.equal(messages(h.session).length, 2);
      assert.deepEqual(messages(h.session)[1]?.details?.deliveryIds, [
        'w-three:1',
        'w-four:1',
      ]);
      assert.match(
        JSON.stringify(h.requests[2]?.messages),
        /Final w-three[\s\S]*Final w-four/,
      );
      assert.equal(
        h.requests.length,
        3,
        'the boundary does not start a duplicate turn',
      );
    } finally {
      await h.close();
    }
  },
);

test(
  'pinned host abort does not revive a run when a child finishes later',
  {timeout: 20_000},
  async () => {
    const h = await setup();
    try {
      const blocked = h.session.prompt('blocked request');
      await until(() => h.requests.length === 1);
      const abort = h.session.abort();
      h.wait.release();
      await abort;
      await blocked;
      h.delivery.outbox.offer(item('w-after-abort'));
      await sleep(250);
      assert.equal(
        h.delivery.outbox.hasPending(),
        false,
        JSON.stringify(messages(h.session)),
      );
      assert.equal(messages(h.session).length, 1);
      assert.equal(h.requests.length, 1, 'the aborted parent remains idle');
      assert.match(
        JSON.stringify(messages(h.session)[0]),
        /Final w-after-abort/,
      );
    } finally {
      await h.close();
    }
  },
);
