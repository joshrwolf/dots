import assert from 'node:assert/strict';
import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import type {AssistantMessage} from '@earendil-works/pi-ai';
import type {ExtensionAPI} from '@earendil-works/pi-coding-agent';
import status from '../../extensions/status/index.ts';
import {formatRecord} from '../../extensions/status/model.ts';
import {publishActivity} from '../../src/host/activity.ts';
import {hostModule} from '../fixtures/host.ts';

const host: typeof import('@earendil-works/pi-coding-agent') = await import(
  hostModule('pi-coding-agent')
);
const ai: typeof import('@earendil-works/pi-ai') = await import(
  hostModule('pi-ai')
);

test(
  'native parent settlement reports its answer, not a worker completion or the user prompt',
  {timeout: 20000},
  async () => {
    const dir = mkdtempSync(join(tmpdir(), 'dots-status-host-'));
    const writes: string[] = [];
    const pending = new Set<() => void>();
    const errors: unknown[] = [];
    let api: ExtensionAPI | undefined;
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
        pi => {
          api = pi;
          status(
            pi,
            {},
            data => writes.push(data),
            () => true,
            run => {
              pending.add(run);
              return () => {
                pending.delete(run);
              };
            },
          );
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
    let responses = 0;
    runtime.registerProvider('status-fixture', {
      api: 'status-fixture',
      apiKey: 'fixture',
      baseUrl: 'https://example.invalid',
      models: [
        {
          id: 'fixture',
          name: 'Fixture',
          reasoning: false,
          input: ['text'],
          contextWindow: 100000,
          maxTokens: 1000,
          cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0},
        },
      ],
      streamSimple(model) {
        const stream = ai.createAssistantMessageEventStream();
        const message: AssistantMessage = {
          role: 'assistant',
          content: [
            {
              type: 'text',
              text:
                responses++ === 0
                  ? 'Waiting for background results.'
                  : 'Verification passed; ready for review.',
            },
          ],
          api: model.api,
          provider: model.provider,
          model: model.id,
          timestamp: Date.now(),
          stopReason: 'stop',
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
          stream.push({type: 'done', reason: 'stop', message});
          stream.end();
        });
        return stream;
      },
    });
    const model = runtime.getModel('status-fixture', 'fixture');
    assert.ok(model);
    const {session} = await host.createAgentSession({
      cwd: dir,
      agentDir: dir,
      modelRuntime: runtime,
      model,
      settingsManager: settings,
      sessionManager: host.SessionManager.inMemory(dir),
      resourceLoader: loader,
      noTools: 'all',
    });
    const flush = () => {
      const callbacks = [...pending];
      pending.clear();
      for (const run of callbacks) run();
    };
    try {
      await session.bindExtensions({
        mode: 'tui',
        onError: error => errors.push(error),
      });
      assert.ok(api);
      publishActivity(api.events, 'workers', [
        {id: 'worker/fixture', title: 'Fixture worker'},
      ]);
      await session.prompt('User request that must not become completion text');
      flush();
      publishActivity(api.events, 'workers', []);
      flush();
      assert.ok(!writes.some(write => write.includes('state=done')));
      await session.prompt('Inspect the worker result');
      flush();
      assert.equal(
        writes.at(-1),
        formatRecord({
          state: 'done',
          message: 'Verification passed; ready for review.',
        }),
      );
      assert.equal(
        writes.filter(write => write.includes('state=done')).length,
        1,
      );
      assert.deepEqual(errors, []);
    } finally {
      await session.abort();
      await session.extensionRunner.emit({
        type: 'session_shutdown',
        reason: 'quit',
      });
      session.dispose();
      rmSync(dir, {recursive: true, force: true});
    }
  },
);
