// Goal attention through the pinned Pi host, the real goal extension with its
// default dots sources and task stream, and the candidate `dots` CLI on PATH,
// in a disposable home. The model is an in-process fixture provider: an
// answer saved outside Pi must start the coordinator's next turn natively,
// and a goal the session coordinates stays in view without a follow.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdirSync, mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {setTimeout as sleep} from 'node:timers/promises';
import test from 'node:test';
import goal from '../../extensions/goal/index.ts';
import {hostModule} from '../fixtures/host.ts';

const host = await import(hostModule('pi-coding-agent'));
const ai = await import(hostModule('pi-ai'));

const ISOLATED = ['HOME', 'XDG_STATE_HOME', 'XDG_DATA_HOME'];
const INHERITED = ['DOTS_WORKER_ID', 'DOTS_AGENT_OWNER', 'PI_SESSION_ID'];
const NOTICE = 'dots.attention.answer';
const OTHER = 'pi-other-fedcba9876543210';
const VIEWER = 'viewer-fixture';

/** The system prompt the model saw: native sections folded in order. */
function promptSections(context) {
  const sections = {};
  for (const message of context.messages) {
    if (message.role !== 'system') continue;
    for (const [name, value] of Object.entries(message.sections ?? {})) {
      if (value === null) delete sections[name];
      else sections[name] = value;
    }
  }
  return Object.values(sections).join('\n');
}

async function until(what, ready, ms = 20_000) {
  const deadline = Date.now() + ms;
  while (!(await ready())) {
    assert.ok(Date.now() < deadline, `timed out waiting for ${what}`);
    await sleep(25);
  }
}

test(
  'an answer saved outside Pi continues only its unheld coordinator, across reload, compaction and tree',
  {timeout: 120_000},
  async () => {
    const dir = mkdtempSync(join(tmpdir(), 'dots-attention-host-'));
    const saved = [...ISOLATED, ...INHERITED].map(key => [
      key,
      process.env[key],
    ]);
    for (const key of ISOLATED) process.env[key] = join(dir, key);
    for (const key of INHERITED) delete process.env[key];
    const checkout = join(dir, 'checkout');
    let project;
    const cli = (args, owner) =>
      JSON.parse(
        execFileSync(
          'dots',
          ['--json', ...args, '--project', project, '--owner', owner],
          {encoding: 'utf8'},
        ),
      );
    const content = (name, value) => {
      const file = join(dir, name);
      writeFileSync(file, JSON.stringify(value));
      return file;
    };
    let session;
    const errors = [];
    try {
      mkdirSync(process.env.HOME, {recursive: true});
      execFileSync('git', ['init', '--quiet', checkout]);
      project = JSON.parse(
        execFileSync(
          'dots',
          ['--json', 'project', 'init', 'Canary', '--fresh'],
          {cwd: checkout, encoding: 'utf8'},
        ),
      ).project.id;
      const briefs = join(
        process.env.XDG_DATA_HOME,
        'dots/workspaces',
        project,
        'briefs',
      );
      mkdirSync(briefs, {recursive: true});
      writeFileSync(join(briefs, 'brief.md'), '# Canary brief\n');

      const requests = [];
      const signals = [];
      const settings = host.SettingsManager.inMemory({
        compaction: {enabled: false, keepRecentTokens: 10, reserveTokens: 50},
        retry: {enabled: false},
        cacheWarming: 'off',
      });
      const loader = new host.DefaultResourceLoader({
        cwd: checkout,
        agentDir: dir,
        settingsManager: settings,
        noExtensions: true,
        noSkills: true,
        noPromptTemplates: true,
        noThemes: true,
        noContextFiles: true,
        extensionFactories: [
          api => goal(api),
          api => {
            api.events.on('dots:attention', signal => signals.push(signal));
            api.on('session_before_compact', event => ({
              compaction: {
                summary: 'Canary summary',
                firstKeptEntryId: event.preparation.firstKeptEntryId,
                tokensBefore: event.preparation.tokensBefore,
              },
            }));
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
      runtime.registerProvider('attention-canary', {
        api: 'attention-canary',
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
          requests.push({
            system: promptSections(context),
            messages: JSON.stringify(context.messages),
          });
          const stream = ai.createAssistantMessageEventStream();
          const done = {
            role: 'assistant',
            content: [{type: 'text', text: 'Fixture response. '.repeat(40)}],
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
              cost: {
                input: 0,
                output: 0,
                cacheRead: 0,
                cacheWrite: 0,
                total: 0,
              },
            },
          };
          queueMicrotask(() => {
            stream.push({type: 'start', partial: done});
            stream.push({type: 'done', reason: 'stop', message: done});
            stream.end();
          });
          return stream;
        },
      });
      const model = runtime.getModel('attention-canary', 'fixture');
      assert.ok(model);
      ({session} = await host.createAgentSession({
        cwd: checkout,
        agentDir: dir,
        modelRuntime: runtime,
        model,
        settingsManager: settings,
        sessionManager: host.SessionManager.inMemory(checkout),
        resourceLoader: loader,
        noTools: 'builtin',
      }));
      await session.bindExtensions({
        mode: 'rpc',
        onError: error => errors.push(error),
      });
      const owner = process.env.DOTS_AGENT_OWNER;
      assert.match(owner ?? '', /^pi-.+-[0-9a-f]{16}$/, 'the runtime actor');
      const notices = () =>
        session.sessionManager
          .getBranch()
          .filter(
            entry =>
              entry.type === 'custom_message' && entry.customType === NOTICE,
          );
      const idle = () => until('an idle session', () => session.isIdle);

      // Goals, coordination, a mandate and requests, all through the CLI.
      const create = title =>
        cli(
          ['task', 'goal', 'create', title, '--brief', 'briefs/brief.md'],
          owner,
        ).goal.root.id;
      const raise = (id, key, as) =>
        cli(
          [
            'attention',
            'raise',
            id,
            '--key',
            key,
            '--content-file',
            content(`${key}.json`, {
              kind: 'decision',
              title: `Decide ${key}`,
              question: `Which ${key}?`,
              choices: [{id: 'a', label: 'Option A'}],
            }),
          ],
          as,
        ).request.id;
      const answer = id =>
        cli(
          [
            'attention',
            'answer',
            id,
            '--content-file',
            content('answer.json', {
              source: 'ui',
              choice: 'a',
              provenance: 'dots attention view · canary terminal',
            }),
          ],
          VIEWER,
        );
      const owned = create('Owned work');
      const theirs = create('Their work');
      const paused = create('Paused work');
      cli(['task', 'goal', 'coordinate', owned], owner);
      cli(['task', 'goal', 'coordinate', theirs], OTHER);
      cli(['task', 'goal', 'coordinate', paused], owner);
      cli(
        [
          'attention',
          'decide',
          owned,
          '--content-file',
          content('mandate.json', {
            kind: 'mandate',
            decision: 'Ship the canary behind a flag.',
            human: {
              text: 'ship it behind a flag',
              source: 'chat',
              provenance: 'pi session canary · interactive input',
            },
          }),
        ],
        owner,
      );
      const mine = raise(owned, 'mine', owner);
      const other = raise(theirs, 'theirs', OTHER);
      const held = raise(paused, 'held', owner);
      cli(['task', 'hold', paused, '--reason', 'Paused by the person'], owner);
      answer(other);
      answer(held);

      // Following another goal reads responsibilities and starts their streams;
      // unfollowing a coordinated goal does not take it out of view.
      await session.prompt(`/goal follow ${owned}`);
      await session.prompt(`/goal follow ${theirs}`);
      await session.prompt(`/goal unfollow ${owned}`);
      const follows = session.sessionManager
        .getBranch()
        .findLast(
          entry =>
            entry.type === 'custom' && entry.customType === 'dots.goal.focus',
        ).data.followed;
      assert.deepEqual(
        follows.map(item => item.id),
        [theirs],
        'the branch records only the follow',
      );
      await until('the coordinator alert', () =>
        signals.some(signal => signal.requests.some(item => item.id === mine)),
      );
      assert.ok(
        signals.every(signal =>
          signal.requests.every(item => item.id === mine),
        ),
        'neither a follower nor a paused goal alerts',
      );

      // An answer saved outside Pi starts the coordinator's next turn.
      await session.prompt('before the answer');
      await idle();
      const before = requests.length;
      answer(mine);
      await until('native continuation', () => requests.length > before);
      await idle();
      assert.deepEqual(
        notices().map(entry => entry.details),
        [{requests: [mine]}],
        'only the unheld coordinator is told, once',
      );
      const turn = requests.at(-1);
      assert.match(turn.messages, new RegExp(`Request ${mine}`));
      assert.doesNotMatch(turn.messages, /not follow/);
      const context = system => {
        assert.match(
          system,
          new RegExp(
            `Goal ${owned} · project ${project} · Owned work \\[open\\]`,
          ),
          'the coordinated goal keeps its tasks in context',
        );
        assert.doesNotMatch(system, /not followed/);
        assert.match(system, /Ship the canary behind a flag\./);
        assert.match(
          system,
          /Human input \(chat, pi session canary · interactive input\): ship it behind a flag/,
        );
        assert.match(
          system,
          new RegExp(`Saved answer, not yet incorporated — ${mine}`),
        );
        assert.match(
          system,
          new RegExp(`Coordinated by ${OTHER}, not this session`),
        );
        assert.equal(
          system.match(/Canary brief/g)?.length,
          2,
          'the coordinated and the followed goal each keep their brief',
        );
        assert.doesNotMatch(
          system,
          new RegExp(paused),
          'a paused goal is not owned',
        );
      };
      context(turn.system);
      assert.ok(
        signals.at(-1).requests.length === 0,
        'an answered request no longer alerts',
      );

      // Reload keeps the runtime; the branch receipt prevents a repeat.
      await session.reload();
      assert.equal(process.env.DOTS_AGENT_OWNER, owner);
      await session.prompt('after reload');
      await idle();
      assert.equal(notices().length, 1, 'reload does not repeat');
      context(requests.at(-1).system);

      // Compaction drops nothing the store holds.
      await session.compact();
      await session.prompt('after compaction');
      await idle();
      assert.equal(notices().length, 1);
      context(requests.at(-1).system);

      // A branch that was never told is told once; the saved answer stays.
      const first = session.sessionManager
        .getBranch()
        .find(
          entry => entry.type === 'message' && entry.message.role === 'user',
        );
      assert.ok(first);
      const told = requests.length;
      await session.navigateTree(first.id, {summarize: false});
      await until(
        'native continuation on the new branch',
        () => requests.length > told && notices().length === 1,
      );
      await idle();
      await sleep(200);
      assert.equal(notices().length, 1, 'once per branch');
      assert.ok(
        requests
          .slice(told)
          .some(turn => turn.messages.includes(`Request ${mine}`)),
        'the notice reached the model on this branch',
      );
      context(requests.at(-1).system);
      assert.deepEqual(errors, []);
    } finally {
      if (session) {
        await session.abort().catch(() => {});
        await session.extensionRunner.emit({
          type: 'session_shutdown',
          reason: 'quit',
        });
        session.dispose();
      }
      for (const [key, value] of saved) {
        if (value === undefined) delete process.env[key];
        else process.env[key] = value;
      }
      rmSync(dir, {recursive: true, force: true});
    }
  },
);
