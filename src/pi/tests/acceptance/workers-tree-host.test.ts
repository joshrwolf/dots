// Session-scoped worker results through the pinned Pi host: model-issued worker
// tools, real tree navigation and real reload. This file is also the worker:
// re-executed with DOTS_TREE_FIXTURE=child it speaks the worker RPC subset.
import assert from 'node:assert/strict';
import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {setTimeout as sleep} from 'node:timers/promises';
import test from 'node:test';
import type {
  AssistantMessage,
  JsonObject,
  TranscriptContext,
} from '@earendil-works/pi-ai';
import type {
  AgentSession,
  CustomMessageEntry,
  ExtensionAPI,
  SessionEntry,
} from '@earendil-works/pi-coding-agent';
import workers from '../../extensions/workers/index.ts';
import {hostModule} from '../fixtures/host.ts';

/** A worker child: starts a run per prompt and settles it after DOTS_TREE_DELAY ms with a neutral final answer. */
function child(): void {
  const args = process.argv.slice(2);
  const sessionFile = args.includes('--session')
    ? args[args.indexOf('--session') + 1]
    : join(
        args[args.indexOf('--session-dir') + 1] ?? tmpdir(),
        'session.jsonl',
      );
  const delay = Number(process.env.DOTS_TREE_DELAY ?? '0');
  const out = (record: object) =>
    process.stdout.write(`${JSON.stringify(record)}\n`);
  let streaming = false;
  let buffer = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk: string) => {
    buffer += chunk;
    for (
      let index = buffer.indexOf('\n');
      index !== -1;
      index = buffer.indexOf('\n')
    ) {
      const command = JSON.parse(buffer.slice(0, index)) as {
        id: string;
        type: string;
        message?: string;
      };
      buffer = buffer.slice(index + 1);
      const ok = (data: object) =>
        out({
          id: command.id,
          type: 'response',
          command: command.type,
          success: true,
          data,
        });
      if (command.type === 'get_state') {
        ok({sessionFile, isStreaming: streaming});
        continue;
      }
      if (command.type !== 'prompt') {
        ok({});
        continue;
      }
      ok({disposition: streaming ? 'queued' : 'started'});
      if (streaming) continue;
      streaming = true;
      out({type: 'agent_start'});
      setTimeout(() => {
        out({
          type: 'message_end',
          message: {
            role: 'assistant',
            stopReason: 'stop',
            content: [
              {
                type: 'text',
                text: `neutral worker result for ${command.message}`,
              },
            ],
          },
        });
        streaming = false;
        out({type: 'agent_settled', aborted: false});
      }, delay);
    }
  });
  process.stdin.on('end', () => process.exit(0));
}

if (process.env.DOTS_TREE_FIXTURE === 'child') child();
else {
  const host: typeof import('@earendil-works/pi-coding-agent') = await import(
    hostModule('pi-coding-agent')
  );
  const ai: typeof import('@earendil-works/pi-ai') = await import(
    hostModule('pi-ai')
  );

  type Call = {name: string; arguments: JsonObject};
  /** The parent model's script: what to call after the latest message, or undefined to answer with text. */
  type Script = (
    last: TranscriptContext['messages'][number] | undefined,
    worker: string | undefined,
    context: TranscriptContext,
  ) => Call | undefined;

  async function until(predicate: () => boolean, ms = 10_000) {
    const deadline = Date.now() + ms;
    while (!predicate()) {
      if (Date.now() > deadline) throw new Error('workers tree host deadline');
      await sleep(10);
    }
  }

  async function setup(script: Script, delayMs: number) {
    const dir = mkdtempSync(join(tmpdir(), 'dots-workers-tree-host-'));
    const saved = {...process.env};
    process.env.PI_CODING_AGENT_DIR = dir;
    process.env.DOTS_TREE_FIXTURE = 'child';
    process.env.DOTS_TREE_DELAY = String(delayMs);
    for (const key of [
      'DOTS_WORKER_ID',
      'DOTS_WORKER_ROOT',
      'DOTS_WORKER_DEPTH',
      'REX_SESSION',
      'REX_BLOCK',
    ])
      delete process.env[key];
    const errors: unknown[] = [];
    const settings = host.SettingsManager.inMemory({
      compaction: {enabled: false},
      retry: {enabled: false},
      cacheWarming: 'off',
      defaultThinkingLevel: 'off',
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
          workers(api, {
            command: [process.execPath, fileURLToPath(import.meta.url)],
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
    let worker: string | undefined;
    runtime.registerProvider('workers-tree-fixture', {
      api: 'workers-tree-fixture',
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
        const last = context.messages.at(-1);
        // An immediate completion may follow the spawn receipt before this request.
        const spawn = context.messages.findLast(
          message =>
            message.role === 'toolResult' &&
            message.toolName === 'worker_spawn',
        );
        if (spawn?.role === 'toolResult') {
          assert.equal(spawn.isError, false, JSON.stringify(spawn.content));
          worker = /Spawned (w-[0-9a-f]{16})/.exec(
            JSON.stringify(spawn.content),
          )?.[1];
        }
        const call = script(last, worker, context);
        const reason = call ? ('toolUse' as const) : ('stop' as const);
        const content: AssistantMessage['content'] = call
          ? [
              {
                type: 'toolCall',
                id: `call-${Date.now()}`,
                name: call.name,
                arguments: call.arguments,
              },
            ]
          : [{type: 'text', text: 'noted'}];
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
    const model = runtime.getModel('workers-tree-fixture', 'fixture');
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
      dir,
      errors,
      worker: () => worker,
      async close() {
        await session.abort();
        await session.extensionRunner.emit({
          type: 'session_shutdown',
          reason: 'quit',
        });
        session.dispose();
        process.env = saved;
        rmSync(dir, {recursive: true, force: true});
        assert.deepEqual(errors, []);
      },
    };
  }

  type Event = CustomMessageEntry<{deliveryIds?: string[]; origin?: string}>;
  const isEvent = (entry: SessionEntry): entry is Event =>
    entry.type === 'custom_message' && entry.customType === 'dots.worker.event';
  const delivered = (session: AgentSession) =>
    session.sessionManager
      .getBranch()
      .filter(isEvent)
      .flatMap(entry => entry.details?.deliveryIds ?? []);
  const journal = (session: AgentSession) =>
    session.sessionManager
      .getEntries()
      .flatMap(entry =>
        entry.type === 'custom' && entry.customType === 'dots.worker.outbox'
          ? [entry.data as {action: string}]
          : [],
      )
      .map(data => data.action);
  const idle = async (session: AgentSession) => {
    await sleep(300);
    await until(() => !session.isStreaming);
  };
  const text = (message: TranscriptContext['messages'][number] | undefined) =>
    JSON.stringify(message?.content ?? '');

  test(
    'results returned to blocked waiters are session-wide: /tree offers them once, reload agrees',
    {timeout: 30_000},
    async () => {
      // spawn -> worker_wait while the child still runs -> message { wait } -> done.
      const h = await setup((last, worker) => {
        if (last?.role === 'user' && text(last).includes('start-fixture'))
          return {
            name: 'worker_spawn',
            arguments: {
              label: 'tree',
              task: 'first',
              model: 'inherit',
              thinking: 'inherit',
            },
          };
        if (last?.role === 'toolResult' && last.toolName === 'worker_spawn')
          return {
            name: 'worker_wait',
            arguments: {id: worker ?? '', timeoutMs: 5000},
          };
        if (last?.role === 'toolResult' && last.toolName === 'worker_wait')
          return {
            name: 'message',
            arguments: {
              to: worker ?? '',
              text: 'second',
              wait: true,
              timeoutMs: 5000,
            },
          };
        return undefined;
      }, 600);
      try {
        const anchor = h.session.sessionManager.appendCustomEntry(
          'fixture-anchor',
          {},
        );
        await h.session.prompt('start-fixture');
        await idle(h.session);
        const id = h.worker();
        assert.ok(id);
        const results = h.session.messages
          .filter(message => message.role === 'toolResult')
          .map(message => [message.toolName, text(message)]);
        assert.match(
          results.find(([name]) => name === 'worker_wait')?.[1] ?? '',
          /completed[\s\S]*neutral worker result for first/,
          'the early waiter returned the result',
        );
        assert.match(
          results.find(([name]) => name === 'message')?.[1] ?? '',
          /completed[\s\S]*neutral worker result for second/,
          'so did message { wait }',
        );
        assert.deepEqual(
          journal(h.session),
          ['add', 'consumed', 'add', 'consumed'],
          'journaled for the session, consumed on this branch',
        );
        assert.deepEqual(
          delivered(h.session),
          [],
          'no second notification on the waiting branch',
        );
        const original = h.session.sessionManager.getLeafId();
        assert.ok(original);

        // A branch that never saw those tool results receives them once, marked with their origin.
        await h.session.navigateTree(anchor);
        await until(() => delivered(h.session).length === 2);
        await idle(h.session);
        assert.deepEqual(delivered(h.session).sort(), [`${id}:1`, `${id}:2`]);
        const offer = h.session.sessionManager
          .getBranch()
          .filter(isEvent)
          .at(-1);
        assert.match(
          JSON.stringify(offer?.content),
          /from another branch of this session/,
        );
        await h.session.reload();
        await idle(h.session);
        assert.deepEqual(
          delivered(h.session).sort(),
          [`${id}:1`, `${id}:2`],
          'reload on that branch delivers nothing again',
        );

        // Back on the branch that waited: its tool results remain the receipts.
        await h.session.navigateTree(original);
        await idle(h.session);
        assert.deepEqual(delivered(h.session), []);
        await h.session.reload();
        await idle(h.session);
        assert.deepEqual(
          delivered(h.session),
          [],
          'reload agrees with staying put',
        );
      } finally {
        await h.close();
      }
    },
  );

  test(
    'a late waiter consumes only on its branch',
    {timeout: 30_000},
    async () => {
      // The child settles at once; worker_wait reads an already-finished worker.
      const h = await setup((last, worker, context) => {
        if (last?.role === 'user' && text(last).includes('start-fixture'))
          return {
            name: 'worker_spawn',
            arguments: {
              label: 'late',
              task: 'quick',
              model: 'inherit',
              thinking: 'inherit',
            },
          };
        const hasResult = (name: string) =>
          context.messages.some(
            message =>
              message.role === 'toolResult' && message.toolName === name,
          );
        if (hasResult('worker_spawn') && !hasResult('worker_wait'))
          return {
            name: 'worker_wait',
            arguments: {id: worker ?? '', timeoutMs: 5000},
          };
        return undefined;
      }, 0);
      try {
        const anchor = h.session.sessionManager.appendCustomEntry(
          'fixture-anchor',
          {},
        );
        await h.session.prompt('start-fixture');
        await idle(h.session);
        const id = h.worker();
        assert.ok(id);
        assert.ok(
          h.session.messages.some(
            message =>
              message.role === 'toolResult' &&
              message.toolName === 'worker_wait' &&
              !message.isError,
          ),
        );
        const onWaitingBranch = delivered(h.session).length;
        assert.ok(
          onWaitingBranch <= 1,
          'delivered at most once where it was waited for',
        );
        assert.equal(
          journal(h.session).filter(action => action === 'add').length,
          1,
          'journaled once for the session',
        );
        // Either the boundary delivered it here first, or the wait consumed it here.
        assert.equal(
          onWaitingBranch +
            journal(h.session).filter(action => action === 'consumed').length,
          1,
        );
        await h.session.navigateTree(anchor);
        await until(() => delivered(h.session).length > 0);
        await idle(h.session);
        assert.deepEqual(
          delivered(h.session),
          [`${id}:1`],
          'offered once on the other branch',
        );
        await h.session.reload();
        await idle(h.session);
        assert.deepEqual(delivered(h.session), [`${id}:1`]);
      } finally {
        await h.close();
      }
    },
  );
  test("a broken newer journal record of a known key never restores the older result (Pi's own session manager)", async () => {
    const {CompletionOutbox} =
      await import('../../extensions/workers/outbox.ts');
    const manager = host.SessionManager.inMemory(tmpdir());
    const owner = manager.getSessionId();
    const item = {
      key: 'w-review:1',
      content: 'Worker review completed\n\nold result',
      details: {id: 'w-review', kind: 'completed'},
    };
    manager.appendCustomEntry('dots.worker.outbox', {
      version: 1,
      owner,
      action: 'add',
      item,
    });
    manager.appendCustomEntry('dots.worker.outbox', {
      version: 1,
      owner,
      action: 'add',
      item: {...item, content: 42},
    });
    const box = new CompletionOutbox(
      {
        appendEntry: (type: string, data: unknown) => {
          manager.appendCustomEntry(type, data);
        },
      },
      () => {},
      () => false,
    );
    assert.match(
      box.restore(owner, manager.getEntries(), manager.getBranch()) ?? '',
      /\/item\/content must be string/,
    );
    assert.equal(box.draft(), undefined);
    // A malformed consumption names its key: this branch withholds it, a branch without it does not.
    manager.appendCustomEntry('dots.worker.outbox', {
      version: 1,
      owner,
      action: 'add',
      item: {...item, content: 'Worker review completed\n\nnew result'},
    });
    const anchor = manager.getLeafId();
    manager.appendCustomEntry('dots.worker.outbox', {
      version: 1,
      owner,
      action: 'consumed',
      key: item.key,
      stray: true,
    });
    box.restore(owner, manager.getEntries(), manager.getBranch());
    assert.equal(box.draft(), undefined);
    assert.ok(anchor);
    manager.branch(anchor);
    box.restore(owner, manager.getEntries(), manager.getBranch());
    assert.match(box.draft()?.content ?? '', /new result/);
    box.close();
  });

  test("a damaged action discriminant does not restore the older result either (Pi's own session manager)", async () => {
    const {CompletionOutbox} =
      await import('../../extensions/workers/outbox.ts');
    for (const damaged of [
      {action: 'invalid-add', item: {content: 42}},
      {action: 'invalid-consumed'},
    ]) {
      const manager = host.SessionManager.inMemory(tmpdir());
      const owner = manager.getSessionId();
      const item = {
        key: 'w-review:1',
        content: 'Worker review completed\n\nold result',
        details: {id: 'w-review', kind: 'completed'},
      };
      manager.appendCustomEntry('dots.worker.outbox', {
        version: 1,
        owner,
        action: 'add',
        item,
      });
      const anchor = manager.getLeafId();
      manager.appendCustomEntry('dots.worker.outbox', {
        version: 1,
        owner,
        ...damaged,
        ...('item' in damaged
          ? {item: {...item, ...damaged.item}}
          : {key: item.key}),
      });
      const box = new CompletionOutbox(
        {
          appendEntry: (type: string, data: unknown) => {
            manager.appendCustomEntry(type, data);
          },
        },
        () => {},
        () => false,
      );
      assert.match(
        box.restore(owner, manager.getEntries(), manager.getBranch()) ?? '',
        /incompatible/,
      );
      assert.equal(box.draft(), undefined, damaged.action);
      assert.ok(anchor);
      manager.branch(anchor);
      box.restore(owner, manager.getEntries(), manager.getBranch());
      // A damaged result payload is gone session-wide; a damaged receipt only withholds on its own branch.
      if (damaged.action === 'invalid-add')
        assert.equal(box.draft(), undefined);
      else assert.match(box.draft()?.content ?? '', /old result/);
      box.close();
    }
  });
}
