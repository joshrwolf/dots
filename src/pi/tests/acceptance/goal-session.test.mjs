// Real host session contexts: shell identity must follow resume/switch, not inherited metadata.
import assert from 'node:assert/strict';
import {execFile, execFileSync} from 'node:child_process';
import {mkdirSync, mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname, join} from 'node:path';
import test from 'node:test';
import {fileURLToPath} from 'node:url';

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

const runtimeOf = session => new RegExp(`^pi-${session}-[0-9a-f]{16}$`);

test(
  'each live runtime is its own actor in model bash, ! commands and jobs, and ending one releases only its claims',
  {timeout: 120_000},
  async () => {
    const dir = mkdtempSync(join(tmpdir(), 'dots-agent-runtime-'));
    // Only what the host, its shell and dots need; no credentials or identity.
    const env = Object.fromEntries(
      ['PATH', 'TMPDIR', 'TERM', 'LANG']
        .filter(key => process.env[key] !== undefined)
        .map(key => [key, process.env[key]]),
    );
    // The child resolves the same host without the caller's mise home.
    env.PI_TEST_HOST_ROOT = dirname(
      dirname(dirname(fileURLToPath(hostModule('pi-coding-agent')))),
    );
    for (const name of [
      'HOME',
      'XDG_STATE_HOME',
      'XDG_DATA_HOME',
      'PI_CODING_AGENT_DIR',
    ]) {
      env[name] = join(dir, name);
      mkdirSync(env[name], {recursive: true});
    }
    const cwd = join(dir, 'checkout');
    const json = (args, options = {}) =>
      JSON.parse(
        execFileSync('dots', ['--json', '--directory', cwd, ...args], {
          encoding: 'utf8',
          env,
          ...options,
        }),
      );
    const fixture = fileURLToPath(
      new URL('../fixtures/agent-runtime.mjs', import.meta.url),
    );
    const run = (args, extra = {}) =>
      new Promise((resolve, reject) =>
        execFile(
          process.execPath,
          [fixture, ...args],
          {env: {...env, ...extra}, cwd, encoding: 'utf8', timeout: 60_000},
          (error, stdout, stderr) =>
            error
              ? reject(new Error(`${error.message}\n${stderr}`, {cause: error}))
              : resolve(
                  Object.fromEntries(
                    stdout
                      .trim()
                      .split('\n')
                      .map(line => JSON.parse(line))
                      .map(line => [line.step, line]),
                  ),
                ),
        ),
      );
    try {
      execFileSync('git', ['init', '--quiet', cwd]);
      const project = json(['project', 'init', 'Fixture', '--fresh']).project
        .id;
      const add = title =>
        json(['task', 'add', title, '--project', project, '--owner', 'fixture'])
          .task.id;
      const first = add('First');
      const second = add('Second');
      // Stale identity inherited from an outer process never stands.
      const steps = await run(['lifecycle', cwd, project, first, second], {
        PI_SESSION_ID: 'outer-session',
        DOTS_AGENT_OWNER: 'pi-outer-session-0123456789abcdef',
      });
      for (const step of [
        'startup',
        'reload',
        'tree',
        'new',
        'resume',
        'fork',
      ]) {
        const {session, shell, user, job} = steps[step];
        // Every child agrees on the true session and its runtime's owner.
        assert.equal(shell.session, session, step);
        assert.match(shell.owner, runtimeOf(session), step);
        assert.deepEqual(user, shell, `${step} !`);
        assert.deepEqual(job, shell, `${step} job`);
      }
      const owner = step => steps[step].shell.owner;
      assert.equal(owner('reload'), owner('startup'));
      assert.equal(owner('tree'), owner('startup'));
      assert.equal(steps['after-reload'].owner, owner('startup'));
      // Ending a runtime released its own claim, not the next runtime's.
      assert.equal(steps['after-new'].owner, null);
      assert.equal(steps['after-resume'].owner, null);
      assert.notEqual(steps.new.session, steps.startup.session);
      // Back to the first session in one process is a new runtime.
      assert.equal(steps.resume.session, steps.startup.session);
      assert.notEqual(owner('resume'), owner('startup'));
      assert.notEqual(steps.fork.session, steps.startup.session);
      assert.equal(
        new Set(['startup', 'new', 'resume', 'fork'].map(owner)).size,
        4,
      );

      // Two processes resuming one session are different actors, and a new
      // process never adopts the owner it inherited from its parent.
      const file = steps.file.file;
      const [one, two, nested] = await Promise.all([
        run(['observe', cwd, file]),
        run(['observe', cwd, file]),
        run(['observe', cwd, file], {
          PI_SESSION_ID: steps.startup.session,
          DOTS_AGENT_OWNER: owner('startup'),
        }),
      ]);
      const observed = [one, two, nested].map(steps => steps.observe);
      for (const {session, shell, user, job} of observed) {
        assert.equal(session, steps.startup.session);
        assert.equal(shell.session, session);
        assert.match(shell.owner, runtimeOf(session));
        assert.deepEqual(user, shell);
        assert.deepEqual(job, shell);
      }
      assert.equal(
        new Set([owner('startup'), ...observed.map(({shell}) => shell.owner)])
          .size,
        4,
      );
    } finally {
      rmSync(dir, {recursive: true, force: true});
    }
  },
);
