// Turn-requesting custom messages (review reply notices, wake fires) through
// the pinned Pi host: an explicit cancellation never starts another run or
// delivers twice, and compaction is never overlapped by a new run.
import assert from 'node:assert/strict';
import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {setTimeout as sleep} from 'node:timers/promises';
import test from 'node:test';
import type {AssistantMessage} from '@earendil-works/pi-ai';
import type {AgentSession, ExtensionAPI} from '@earendil-works/pi-coding-agent';
import review from '../../extensions/review/index.ts';
import wake, {type Clock} from '../../extensions/wake/index.ts';
import {hostModule} from '../fixtures/host.ts';

const host: typeof import('@earendil-works/pi-coding-agent') = await import(
  hostModule('pi-coding-agent')
);
const ai: typeof import('@earendil-works/pi-ai') = await import(
  hostModule('pi-ai')
);

const REVIEW = 'r-00000001';

/** Timers fire only when the test advances time. */
function manualClock(): Clock & {advance(ms: number): Promise<void>} {
  let now = Date.UTC(2026, 0, 1);
  let sequence = 0;
  const timers = new Map<number, {at: number; fn: () => void}>();
  return {
    now: () => now,
    setTimeout: (fn, ms) => {
      timers.set(++sequence, {at: now + ms, fn});
      return sequence;
    },
    clearTimeout: handle => {
      if (typeof handle === 'number') timers.delete(handle);
    },
    async advance(ms) {
      const target = now + ms;
      for (;;) {
        const due = [...timers.entries()]
          .filter(([, timer]) => timer.at <= target)
          .sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        now = due[1].at;
        timers.delete(due[0]);
        due[1].fn();
        await sleep(20);
      }
      now = target;
    },
  };
}

interface Fixture {
  session: AgentSession;
  /** Model requests so far. */
  requests(): number;
  /** Custom messages of `customType` on the branch. */
  receipts(customType: string): number;
  /** The user replies in the review; its directory watcher reports the change. */
  reply(): void;
  clock: ReturnType<typeof manualClock>;
  /** Resolves once compaction or a summarized /tree waits on `release()`. */
  held(): Promise<void>;
  release(): void;
  close(): Promise<void>;
}

/**
 * A native session with the review and wake extensions over fake review
 * state. Prompt `hold` streams until aborted; `set-wake` sets a recurring
 * wake; everything else answers at once with enough text to compact.
 * Compaction and summarized tree navigation wait until `release()`.
 */
async function fixture(): Promise<Fixture> {
  const dir = mkdtempSync(join(tmpdir(), 'dots-turn-delivery-'));
  const clock = manualClock();
  const threads: unknown[] = [];
  let changed = () => {};
  let release = () => {};
  let entered: () => void = () => {};
  const held = new Promise<void>(resolve => {
    entered = resolve;
  });
  const gate = new Promise<void>(resolve => {
    release = resolve;
  });
  let requests = 0;
  const settings = host.SettingsManager.inMemory({
    compaction: {enabled: false, keepRecentTokens: 10, reserveTokens: 50},
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
    noContextFiles: true,
    extensionFactories: [
      (api: ExtensionAPI) => {
        review(api, {
          settleMs: 5,
          watch: (_directory, onChange) => {
            changed = onChange;
            return {close: () => {}};
          },
          dots: async args =>
            args[1] === 'list'
              ? {reviews: [{id: REVIEW, file: join(dir, `${REVIEW}.json`)}]}
              : {
                  review: {
                    id: REVIEW,
                    worktree: {path: dir, branch: 'main'},
                    threads: structuredClone(threads),
                  },
                },
        });
        wake(api, {clock});
        api.on('session_before_compact', async event => {
          entered();
          await gate;
          return {
            compaction: {
              summary: 'Fixture summary',
              firstKeptEntryId: event.preparation.firstKeptEntryId,
              tokensBefore: event.preparation.tokensBefore,
            },
          };
        });
        api.on('session_before_tree', async () => {
          entered();
          await gate;
          return {summary: {summary: 'Fixture tree summary'}};
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
  runtime.registerProvider('turn-delivery-fixture', {
    api: 'turn-delivery-fixture',
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
    streamSimple(model, context, options) {
      requests++;
      const stream = ai.createAssistantMessageEventStream();
      const last = context.messages.at(-1);
      const prompt =
        last?.role === 'user'
          ? typeof last.content === 'string'
            ? last.content
            : JSON.stringify(last.content)
          : '';
      const call = prompt.includes('set-wake');
      const message = (
        stopReason: AssistantMessage['stopReason'],
      ): AssistantMessage => ({
        role: 'assistant',
        content: call
          ? [
              {
                type: 'toolCall',
                id: 'call-wake',
                name: 'wake',
                arguments: {text: 'tick', every: '1m', limit: 5},
              },
            ]
          : [{type: 'text', text: 'Fixture response. '.repeat(100)}],
        api: model.api,
        provider: model.provider,
        model: model.id,
        timestamp: Date.now(),
        stopReason,
        usage: {
          input: 1,
          output: 1,
          cacheRead: 0,
          cacheWrite: 0,
          totalTokens: 2,
          cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0},
        },
      });
      if (prompt.includes('hold')) {
        options?.signal?.addEventListener(
          'abort',
          () => {
            stream.push({
              type: 'error',
              reason: 'aborted',
              error: message('aborted'),
            });
            stream.end();
          },
          {once: true},
        );
        return stream;
      }
      queueMicrotask(() => {
        const reason = call ? ('toolUse' as const) : ('stop' as const);
        const done = message(reason);
        stream.push({type: 'start', partial: done});
        stream.push({type: 'done', reason, message: done});
        stream.end();
      });
      return stream;
    },
  });
  const model = runtime.getModel('turn-delivery-fixture', 'fixture');
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
  const errors: unknown[] = [];
  await session.bindExtensions({mode: 'rpc', onError: e => errors.push(e)});
  await sleep(50);
  return {
    session,
    clock,
    requests: () => requests,
    receipts: customType =>
      session.sessionManager
        .getBranch()
        .filter(
          entry =>
            entry.type === 'custom_message' && entry.customType === customType,
        ).length,
    reply() {
      threads.push({
        id: `t${threads.length + 1}`,
        anchor: null,
        comments: [{author: 'user', body: 'Please explain', status: null}],
      });
      changed();
    },
    held: () => held,
    release: () => release(),
    async close() {
      // A failed assertion can leave compaction held; abort waits for it.
      release();
      await session.abort();
      await session.extensionRunner.emit({
        type: 'session_shutdown',
        reason: 'quit',
      });
      session.dispose();
      rmSync(dir, {recursive: true, force: true});
      assert.deepEqual(errors, []);
    },
  };
}

/**
 * Starts a run whose model request streams until aborted, and returns it once
 * that request is in flight. Wrapped so awaiting this does not await the run.
 */
async function holding(f: Fixture): Promise<{run: Promise<void>}> {
  const before = f.requests();
  const run = f.session.prompt('hold');
  while (f.requests() === before) await sleep(5);
  return {run};
}

for (const clearQueue of [true, false]) {
  test(
    `a reply during a cancelled run is recorded once and starts nothing (${clearQueue ? 'queue cleared' : 'SDK abort'})`,
    {timeout: 20_000},
    async () => {
      const f = await fixture();
      try {
        const {run} = await holding(f);
        f.reply();
        await sleep(100);
        assert.equal(f.receipts('dots.review.notice'), 0);
        if (clearQueue) f.session.clearQueue();
        await f.session.abort();
        await run;
        await sleep(400);
        assert.equal(f.requests(), 1, 'the cancellation stands');
        assert.equal(f.receipts('dots.review.notice'), 1);

        await f.session.prompt('go on');
        await sleep(100);
        assert.equal(f.requests(), 2);
        assert.equal(f.receipts('dots.review.notice'), 1, 'never repeated');
        f.reply();
        await sleep(400);
        assert.equal(f.requests(), 3, 'a user prompt ends the cancellation');
        assert.equal(f.receipts('dots.review.notice'), 2);
      } finally {
        await f.close();
      }
    },
  );
}

test(
  'a reply during manual compaction waits for it to finish',
  {timeout: 20_000},
  async () => {
    const f = await fixture();
    try {
      await f.session.prompt('seed '.repeat(100));
      const compact = f.session.compact();
      await f.held();
      const before = f.requests();
      f.reply();
      await sleep(400);
      assert.equal(f.session.isCompacting, true);
      assert.equal(f.requests(), before, 'no run overlaps compaction');
      assert.equal(f.receipts('dots.review.notice'), 0);
      f.release();
      await compact;
      await sleep(600);
      assert.equal(f.receipts('dots.review.notice'), 1);
      assert.equal(f.requests(), before + 1, 'delivered once compaction ended');
    } finally {
      await f.close();
    }
  },
);

test(
  'a wake fire during manual compaction or a cancelled run never overlaps or restarts work',
  {timeout: 20_000},
  async () => {
    const f = await fixture();
    try {
      await f.session.prompt('set-wake');
      await f.session.prompt('seed '.repeat(100));
      const compact = f.session.compact();
      await f.held();
      const before = f.requests();
      await f.clock.advance(60_000);
      await sleep(400);
      assert.equal(f.requests(), before, 'no run overlaps compaction');
      assert.equal(f.receipts('dots.wake'), 0);
      f.release();
      await compact;
      await sleep(600);
      assert.equal(f.receipts('dots.wake'), 1);
      assert.equal(f.requests(), before + 1);

      const {run} = await holding(f);
      await f.clock.advance(60_000);
      await sleep(100);
      await f.session.abort();
      await run;
      await sleep(400);
      assert.equal(f.requests(), before + 2, 'the cancellation stands');
      assert.equal(
        f.receipts('dots.wake'),
        2,
        'the fire is recorded, not lost',
      );
      await f.clock.advance(60_000);
      await sleep(400);
      assert.equal(f.receipts('dots.wake'), 3, 'the next fire is not skipped');
      assert.equal(f.requests(), before + 2);
    } finally {
      await f.close();
    }
  },
);

test(
  'a wake fire held through /tree is labelled for the branch it lands on',
  {timeout: 20_000},
  async () => {
    const f = await fixture();
    try {
      await f.session.prompt('seed');
      const target = f.session.sessionManager
        .getBranch()
        .find(
          entry =>
            entry.type === 'message' && entry.message.role === 'assistant',
        );
      assert.ok(target);
      await f.session.prompt('set-wake');
      const navigation = f.session.navigateTree(target.id, {summarize: true});
      await f.held();
      await f.clock.advance(60_000);
      await sleep(100);
      assert.equal(f.receipts('dots.wake'), 0, 'held while summarizing');
      f.release();
      await navigation;
      await sleep(200);
      const branch = f.session.sessionManager.getBranch();
      assert.equal(
        branch.some(
          entry => entry.type === 'custom' && entry.customType === 'dots.wake',
        ),
        false,
        'the wake was set on the branch left behind',
      );
      const receipt = branch.find(
        entry =>
          entry.type === 'custom_message' && entry.customType === 'dots.wake',
      );
      assert.ok(receipt?.type === 'custom_message');
      assert.equal((receipt.details as {elsewhere?: boolean}).elsewhere, true);
      assert.match(
        typeof receipt.content === 'string' ? receipt.content : '',
        / · set on another branch of this session\n/,
      );
    } finally {
      await f.close();
    }
  },
);
