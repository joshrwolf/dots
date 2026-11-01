// Real host session contexts: shell identity must follow resume/switch, not inherited metadata.
import assert from 'node:assert/strict';
import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';

import {hostModule} from '../fixtures/host.ts';
const host = await import(hostModule('pi-coding-agent'));

test(
  'native shell identity survives resume and separates new sessions from inherited parents',
  {timeout: 20_000},
  async () => {
    const dir = mkdtempSync(join(tmpdir(), 'dots-goal-session-'));
    const previous = process.env.PI_SESSION_ID;
    process.env.PI_SESSION_ID = 'inherited-parent';
    const observations = [];
    const errors = [];
    const runtime = await host.ModelRuntime.create({
      authPath: join(dir, 'auth.json'),
      modelsPath: null,
      modelsStorePath: join(dir, 'models.json'),
      allowModelNetwork: false,
      refreshOnCreate: false,
    });
    let session;
    async function create(manager) {
      const settings = host.SettingsManager.inMemory({cacheWarming: 'off'});
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
            const bash = host.createBashToolDefinition(dir);
            pi.registerCommand('fixture-identity', {
              description: 'Observe shell identity',
              async handler(_args, ctx) {
                const result = await bash.execute(
                  'fixture',
                  {command: 'printf \'%s\' "$PI_SESSION_ID"'},
                  undefined,
                  undefined,
                  ctx,
                );
                observations.push({
                  expected: ctx.sessionManager.getSessionId(),
                  actual: result.content.map(part => part.text ?? '').join(''),
                });
                pi.appendEntry('fixture.identity', {});
              },
            });
          },
        ],
      });
      await loader.reload();
      const created = await host.createAgentSession({
        cwd: dir,
        agentDir: dir,
        settingsManager: settings,
        resourceLoader: loader,
        modelRuntime: runtime,
        sessionManager: manager,
        noTools: 'builtin',
      });
      await created.session.bindExtensions({
        mode: 'rpc',
        onError: error => errors.push(error),
      });
      return created.session;
    }
    async function close() {
      await session.extensionRunner.emit({
        type: 'session_shutdown',
        reason: 'quit',
      });
      session.dispose();
      session = undefined;
    }
    try {
      session = await create(
        host.SessionManager.create(dir, join(dir, 'sessions')),
      );
      await session.prompt('/fixture-identity');
      const file = session.sessionFile;
      // SessionManager defers disk creation until a conversation message exists.
      session.sessionManager.appendMessage({
        role: 'user',
        content: 'Identity fixture',
        timestamp: Date.now(),
      });
      await close();
      session = await create(host.SessionManager.open(file));
      await session.prompt('/fixture-identity');
      await close();
      session = await create(
        host.SessionManager.create(dir, join(dir, 'sessions')),
      );
      await session.prompt('/fixture-identity');
      assert.equal(observations.length, 3);
      for (const observation of observations) {
        assert.equal(observation.actual, observation.expected);
        assert.notEqual(observation.actual, 'inherited-parent');
      }
      assert.equal(observations[0].actual, observations[1].actual);
      assert.notEqual(observations[0].actual, observations[2].actual);
      assert.deepEqual(errors, []);
    } finally {
      if (session) await close();
      if (previous === undefined) delete process.env.PI_SESSION_ID;
      else process.env.PI_SESSION_ID = previous;
      rmSync(dir, {recursive: true, force: true});
    }
  },
);
