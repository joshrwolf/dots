import assert from 'node:assert/strict';
import {execFile, execFileSync} from 'node:child_process';
import {
  mkdirSync,
  mkdtempSync,
  realpathSync,
  readFileSync,
  existsSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {promisify} from 'node:util';
import test from 'node:test';
import register from '../../extensions/workers/index.ts';
import {dotsSource} from '../../extensions/goal/source.ts';
import {bindAgent, endAgent} from '../../src/host/agent.ts';
import {inspection} from '../../extensions/goal/present.ts';
import {fixtureModel} from '../fixtures/worker-assignment-provider.mjs';
import {hostPins} from '../fixtures/host.ts';

const exec = promisify(execFile);
const provider = fileURLToPath(
  new URL('../fixtures/worker-assignment-provider.mjs', import.meta.url),
);

// The parent exercises the extension tools directly; every child is the installed
// Pi host with an isolated deterministic provider, real session files and real dots.
function harness(cwd, entries = []) {
  const handlers = {};
  const tools = new Map();
  register({
    // Pi runs every handler of an event, in registration order.
    on(name, handler) {
      const prior = handlers[name];
      handlers[name] = prior
        ? (...args) => {
            const earlier = prior(...args);
            return handler(...args) ?? earlier;
          }
        : handler;
    },
    getSettings: () => ({defaultThinkingLevel: 'off'}),
    registerTool(tool) {
      tools.set(tool.name, tool);
    },
    registerCommand() {},
    registerMessageRenderer() {},
    sendMessage() {},
    events: {on() {}, emit() {}},
    appendEntry(customType, data) {
      entries.push({type: 'custom', customType, data});
    },
    async exec(command, args, options) {
      try {
        const result = await exec(command, args, {
          ...options,
          encoding: 'utf8',
        });
        return {...result, code: 0, killed: false};
      } catch (error) {
        return {
          stdout: error.stdout ?? '',
          stderr: error.stderr ?? error.message,
          code: 1,
          killed: !!error.killed,
        };
      }
    },
  });
  const ctx = {
    cwd,
    hasUI: false,
    mode: 'rpc',
    model: fixtureModel,
    thinkingLevel: 'off',
    scopedModels: [{model: fixtureModel}],
    sessionManager: {
      getBranch: () => entries,
      getEntries: () => entries,
      getSessionId: () => 'session-assignment',
      getSessionFile: () => undefined,
    },
  };
  return {
    entries,
    start: () => handlers.session_start({}, ctx),
    close: () => handlers.session_shutdown(),
    call: (name, params) =>
      tools.get(name).execute(
        'test',
        {
          ...(name === 'worker_spawn'
            ? {model: 'inherit', thinking: 'off'}
            : {}),
          ...params,
        },
        undefined,
        undefined,
        ctx,
      ),
  };
}

test(
  'real Pi children preserve task associations through completion, reload and continuation; only the coordinator dispatches unheld goal work',
  {timeout: 60_000},
  async () => {
    const directory = realpathSync(
      mkdtempSync(path.join(tmpdir(), 'dots-worker-assignment-')),
    );
    const root = path.join(directory, 'repo');
    mkdirSync(root);
    const keys = [
      'PI_CODING_AGENT_DIR',
      'XDG_STATE_HOME',
      'XDG_DATA_HOME',
      'HOME',
      'DOTS_WORKER_ID',
      'DOTS_WORKER_ROOT',
      'DOTS_WORKER_DEPTH',
      'PI_OFFLINE',
      'PI_SESSION_ID',
      'DOTS_AGENT_OWNER',
    ];
    const saved = Object.fromEntries(keys.map(key => [key, process.env[key]]));
    let parent;
    let resumed;
    // This parent session's live runtime, bound as the goal extension would.
    const agent = bindAgent('session-assignment', 'startup', {});
    const coordinator = agent.owner;
    const git = (cwd, args) =>
      execFileSync('git', ['-C', cwd, ...args], {encoding: 'utf8'});
    // Synthetic commits stay independent of user hooks, identity and signing agents.
    const commit = (cwd, message) =>
      git(cwd, [
        '-c',
        'user.name=Fixture',
        '-c',
        'user.email=fixture@example.com',
        '-c',
        'commit.gpgsign=false',
        '-c',
        'core.hooksPath=/dev/null',
        'commit',
        '-m',
        message,
      ]);
    const wt = (cwd, args) =>
      JSON.parse(
        execFileSync('wt', ['-C', cwd, ...args, '--format', 'json'], {
          encoding: 'utf8',
        }),
      );
    let project;
    const task = args =>
      JSON.parse(
        execFileSync(
          'dots',
          ['--json', 'task', '--project', project, ...args],
          {
            cwd: root,
            encoding: 'utf8',
          },
        ),
      );
    try {
      assert.equal(
        execFileSync('pi', ['--version'], {encoding: 'utf8'}).trim(),
        hostPins['@earendil-works/pi-coding-agent'],
      );
      const agent = path.join(root, 'agent');
      mkdirSync(agent);
      process.env.PI_CODING_AGENT_DIR = agent;
      process.env.HOME = directory;
      process.env.XDG_STATE_HOME = path.join(directory, 'state');
      process.env.XDG_DATA_HOME = path.join(directory, 'data');
      process.env.PI_OFFLINE = '1';
      for (const key of [
        'DOTS_WORKER_ID',
        'DOTS_WORKER_ROOT',
        'DOTS_WORKER_DEPTH',
        'PI_SESSION_ID',
        'DOTS_AGENT_OWNER',
      ])
        delete process.env[key];
      writeFileSync(
        path.join(agent, 'settings.json'),
        JSON.stringify({
          extensions: [provider],
          packages: [],
          defaultProvider: fixtureModel.provider,
          defaultModel: fixtureModel.id,
          compaction: {enabled: false},
          retry: {enabled: false},
          cacheWarming: 'off',
        }),
      );
      execFileSync('git', ['init', '--initial-branch=main', root]);
      writeFileSync(path.join(root, 'README.md'), '# Fixture checkout\n');
      git(root, ['add', 'README.md']);
      project = JSON.parse(
        execFileSync(
          'dots',
          [
            '--json',
            '--directory',
            root,
            'project',
            'init',
            'Fixture',
            '--fresh',
          ],
          {encoding: 'utf8'},
        ),
      ).project.id;
      const brief = path.join(
        process.env.XDG_DATA_HOME,
        'dots/workspaces',
        project,
        'briefs/goal.md',
      );
      mkdirSync(path.dirname(brief), {recursive: true});
      writeFileSync(brief, '# Outcome\nVerify local results together.');
      commit(root, 'Fixture base');
      wt(root, [
        'switch',
        '--create',
        'writer',
        '--base',
        'HEAD',
        '--no-hooks',
        '--no-cd',
      ]);
      wt(root, [
        'switch',
        '--create',
        'reviewer',
        '--base',
        'HEAD',
        '--no-hooks',
        '--no-cd',
      ]);
      const checkouts = wt(root, [
        '--config-set',
        'list.full=false',
        '--config-set',
        'list.json-schema=2',
        'list',
      ]);
      const writer = checkouts.items.find(item => item.branch === 'writer')
        .worktree.path;
      const reviewer = checkouts.items.find(item => item.branch === 'reviewer')
        .worktree.path;
      assert.notEqual(writer, reviewer);
      const goal = task([
        'goal',
        'create',
        'Local coordination',
        '--brief',
        'briefs/goal.md',
      ]).goal.root.id;
      task(['goal', 'coordinate', goal, '--owner', coordinator]);
      task([
        'ref',
        'set',
        goal,
        'https://example.com/issues/1',
        '--label',
        'Issue 1',
        '--owner',
        coordinator,
      ]);
      const created = task([
        'add',
        'Verify local result',
        '--parent',
        goal,
        '--owner',
        coordinator,
      ]);
      assert.deepEqual(
        created.task.references,
        [],
        'children do not inherit goal issue-closing intent',
      );
      const assignment = {project: created.project, id: created.task.id};
      parent = harness(root);
      await parent.start();
      await assert.rejects(
        parent.call('worker_spawn', {
          label: 'wrong-project',
          task: 'Inspect',
          assignment: {...assignment, project: 'other-project'},
        }),
        /Task lookup failed/,
      );
      assert.equal(
        parent.entries.length,
        0,
        'preflight failure creates no worker/session entry',
      );
      const first = (
        await parent.call('worker_spawn', {
          label: 'writer',
          task: 'Write result.txt in this checkout, then report task association',
          assignment,
          cwd: writer,
          tools: ['write'],
        })
      ).details;
      const second = (
        await parent.call('worker_spawn', {
          label: 'peer',
          task: 'Write result.txt in this checkout, then report task association',
          assignment,
          cwd: reviewer,
          tools: ['write'],
        })
      ).details;
      for (const worker of [first, second]) {
        const waited = await parent.call('worker_wait', {
          id: worker.id,
          timeoutMs: 20_000,
        });
        assert.equal(waited.details.state, 'completed', JSON.stringify(waited));
        assert.deepEqual(waited.details.assignment, assignment);
        assert.match(waited.details.result, new RegExp(assignment.id));
        assert.ok(waited.details.session);
      }
      assert.equal(first.cwd, writer);
      assert.equal(second.cwd, reviewer);
      const artifact = `Fixture artifact from ${path.basename(writer)}\n`;
      assert.equal(
        readFileSync(path.join(writer, 'result.txt'), 'utf8'),
        artifact,
      );
      assert.equal(
        readFileSync(path.join(reviewer, 'result.txt'), 'utf8'),
        `Fixture artifact from ${path.basename(reviewer)}\n`,
      );
      assert.equal(
        existsSync(path.join(root, 'result.txt')),
        false,
        'overlapping child edits remain isolated',
      );

      // Fresh spawns and continuations of a retained worker are both goal
      // dispatch: each refuses paused, request-held or foreign-coordinated work.
      const refused = async (why, reason) => {
        await assert.rejects(
          parent.call('worker_spawn', {
            label: 'late',
            task: 'Write result.txt in this checkout, then report task association',
            assignment,
            cwd: writer,
          }),
          reason,
          `${why}: fresh spawn`,
        );
        await assert.rejects(
          parent.call('message', {
            to: first.id,
            text: 'Report association again',
            wait: true,
            timeoutMs: 20_000,
          }),
          reason,
          `${why}: continuation`,
        );
        const status = (await parent.call('worker_status', {id: first.id}))
          .details;
        assert.equal(status.state, 'completed', `${why}: worker unchanged`);
      };
      task([
        'hold',
        assignment.id,
        '--reason',
        'Paused',
        '--owner',
        coordinator,
      ]);
      await refused('held task', /is held; resume it before dispatch/);
      task(['reopen', assignment.id, '--owner', coordinator]);
      task(['hold', goal, '--reason', 'Goal paused', '--owner', coordinator]);
      await refused('paused goal', /which has no coordinator/);
      task(['goal', 'coordinate', goal, '--resume', '--owner', coordinator]);
      // Stopping your own worker never needs coordination; a settled one has
      // already closed its process.
      assert.match(
        (await parent.call('worker_stop', {id: second.id})).content[0].text,
        /nothing to stop/,
      );
      assert.equal(
        (await parent.call('worker_status', {id: second.id})).details.state,
        'completed',
      );
      const other = 'pi-session-other-0123456789abcdef';
      task(['goal', 'coordinate', goal, '--take', '--owner', other]);
      await refused('lost coordination', new RegExp(`coordinated by ${other}`));
      task(['goal', 'coordinate', goal, '--take', '--owner', coordinator]);
      assert.equal(
        task(['show', assignment.id]).task.status,
        'open',
        'completion does not accept the task',
      );
      assert.equal(
        task(['show', assignment.id]).task.owner,
        null,
        'association does not claim the task',
      );
      const observed = await dotsSource.show(
        root,
        goal,
        new AbortController().signal,
      );
      assert.equal(observed.workers.warning, undefined);
      assert.deepEqual(
        new Set(observed.workers.rows.map(w => w.id)),
        new Set([first.id, second.id]),
      );
      const rendered = inspection({
        focus: {id: goal, project},
        snapshot: observed,
      });
      assert.match(rendered, /Contributing workers/);
      assert.match(rendered, /Recorded checkout:/);
      assert.match(
        rendered,
        /Reference: Issue 1 https:\/\/example.com\/issues\/1/,
      );
      task(['claim', assignment.id, '--owner', coordinator]);
      task([
        'review',
        assignment.id,
        '--owner',
        coordinator,
        '--result',
        'Both contributors completed; combined acceptance remains.',
      ]);
      assert.throws(
        () =>
          task(['claim', '--next', '--parent', goal, '--owner', coordinator]),
        /Command failed/,
      );
      assert.equal(
        (await dotsSource.show(root, goal, new AbortController().signal))
          .overview.progress.review,
        1,
      );
      await parent.close();
      resumed = harness(root, parent.entries);
      await resumed.start();
      const restored = (await resumed.call('worker_status', {id: first.id}))
        .details;
      assert.deepEqual(restored.assignment, assignment);
      const continued = await resumed.call('message', {
        to: first.id,
        text: 'Report association again',
        wait: true,
        timeoutMs: 20_000,
      });
      assert.equal(
        continued.details.state,
        'completed',
        JSON.stringify(continued),
      );
      assert.deepEqual(continued.details.assignment, assignment);
      assert.equal(continued.details.session, restored.session);
      assert.match(continued.details.result, new RegExp(assignment.id));
      assert.match(
        (await resumed.call('worker_status', {id: first.id})).content[0].text,
        /checkout:.*dots-worker-assignment-/,
      );
      assert.equal(
        task(['show', assignment.id]).task.status,
        'review',
        'continuation does not accept the handoff',
      );
      task(['claim', assignment.id, '--owner', coordinator]);
      git(writer, ['add', 'result.txt']);
      commit(writer, 'Fixture result');
      wt(writer, [
        'merge',
        'main',
        '--no-commit',
        '--no-rebase',
        '--no-remove',
        '--no-hooks',
      ]);
      assert.equal(
        readFileSync(path.join(root, 'result.txt'), 'utf8'),
        artifact,
      );
      assert.notEqual(
        readFileSync(path.join(reviewer, 'result.txt'), 'utf8'),
        artifact,
        "integration preserves the other contributor's WIP",
      );
      assert.equal(
        git(root, ['rev-parse', 'HEAD']),
        git(writer, ['rev-parse', 'HEAD']),
      );
      task([
        'done',
        assignment.id,
        '--owner',
        coordinator,
        '--result',
        'Verified local integration, two contributors and restored session association.',
      ]);
      const accepted = await dotsSource.show(
        root,
        goal,
        new AbortController().signal,
        true,
      );
      assert.equal(accepted.overview.progress.done, 1);
      assert.equal(
        accepted.overview.goal.root.status,
        'open',
        'combined goal acceptance remains explicit',
      );
    } finally {
      await resumed?.close();
      await parent?.close();
      endAgent(agent, {});
      for (const key of keys) {
        if (saved[key] === undefined) delete process.env[key];
        else process.env[key] = saved[key];
      }
      rmSync(directory, {recursive: true, force: true});
    }
  },
);
