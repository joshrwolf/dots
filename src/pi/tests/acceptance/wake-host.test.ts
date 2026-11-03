// Wake restoration through the pinned Pi host: model-issued wake tools, real
// session entries and real reload, with an injected clock and check runner.
import assert from 'node:assert/strict';
import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {setTimeout as sleep} from 'node:timers/promises';
import test from 'node:test';
import type {AssistantMessage, JsonObject} from '@earendil-works/pi-ai';
import type {ExtensionAPI} from '@earendil-works/pi-coding-agent';
import wake, {type Clock} from '../../extensions/wake/index.ts';
import {hostModule} from '../fixtures/host.ts';

const host: typeof import('@earendil-works/pi-coding-agent') = await import(
  hostModule('pi-coding-agent')
);
const ai: typeof import('@earendil-works/pi-ai') = await import(
  hostModule('pi-ai')
);

/** Timers fire only when the test advances time. */
function manualClock(): Clock & {
  advance(ms: number): Promise<void>;
  pending(): number;
} {
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
    pending: () => timers.size,
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

test(
  'an unreadable latest wake record never re-arms the older timer across a real reload',
  {timeout: 20_000},
  async () => {
    const dir = mkdtempSync(join(tmpdir(), 'dots-wake-host-'));
    const clock = manualClock();
    const checks: string[] = [];
    const run = async (command: string) => {
      checks.push(command);
      return {stdout: '', stderr: '', code: 0};
    };
    const errors: unknown[] = [];
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
          wake(api, {clock, run});
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
    // The model sets two wakes when asked, lists them when asked, and otherwise answers.
    const script = (
      prompt: string,
      calls: number,
    ): {name: string; arguments: JsonObject} | undefined => {
      if (prompt.includes('set-wakes') && calls === 0)
        return {
          name: 'wake',
          arguments: {
            text: 'older check',
            in: '1s',
            check: 'fixture-check',
            on: 'always',
          },
        };
      if (prompt.includes('set-wakes') && calls === 1)
        return {name: 'wake', arguments: {text: 'kept', in: '5m'}};
      if (prompt.includes('list-wakes') && calls === 0)
        return {name: 'wake_list', arguments: {}};
      return undefined;
    };
    runtime.registerProvider('wake-host-fixture', {
      api: 'wake-host-fixture',
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
        const userIndex = context.messages.findLastIndex(
          message => message.role === 'user',
        );
        const user = context.messages[userIndex];
        const prompt =
          typeof user?.content === 'string'
            ? user.content
            : JSON.stringify(user?.content ?? '');
        const calls = context.messages
          .slice(userIndex)
          .filter(message => message.role === 'toolResult').length;
        const call = script(prompt, calls);
        const reason = call ? ('toolUse' as const) : ('stop' as const);
        const content: AssistantMessage['content'] = call
          ? [
              {
                type: 'toolCall',
                id: `call-${calls}`,
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
    const model = runtime.getModel('wake-host-fixture', 'fixture');
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
    try {
      await session.prompt('set-wakes');
      const records = session.sessionManager.getEntries().flatMap(entry =>
        entry.type === 'custom' && entry.customType === 'dots.wake'
          ? [
              entry.data as {
                id: string;
                text: string;
                trigger: {check?: object};
              },
            ]
          : [],
      );
      const older = records.find(record => record.text === 'older check');
      const kept = records.find(record => record.text === 'kept');
      assert.ok(older && kept);
      // A newer record of the same wake that this runtime cannot read.
      session.sessionManager.appendCustomEntry('dots.wake', {
        ...older,
        active: false,
        trigger: {
          ...older.trigger,
          check: {...older.trigger.check, on: 'invalid-policy'},
        },
      });
      await session.reload();
      await clock.advance(2_000);
      assert.deepEqual(checks, [], 'the older check never runs');
      assert.equal(
        session.messages.filter(
          message =>
            message.role === 'custom' && message.customType === 'dots.wake',
        ).length,
        0,
        'and never fires',
      );
      assert.equal(clock.pending(), 1, 'only the readable wake is armed');
      await session.prompt('list-wakes');
      const result = session.messages.findLast(
        message =>
          message.role === 'toolResult' && message.toolName === 'wake_list',
      );
      const listed = JSON.stringify(
        result?.role === 'toolResult' ? result.content : undefined,
      );
      assert.match(listed, new RegExp(kept.id));
      assert.doesNotMatch(listed, new RegExp(older.id));
    } finally {
      await session.abort();
      await session.extensionRunner.emit({
        type: 'session_shutdown',
        reason: 'quit',
      });
      session.dispose();
      rmSync(dir, {recursive: true, force: true});
    }
    assert.deepEqual(errors, []);
  },
);
