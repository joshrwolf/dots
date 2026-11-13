// Task wakes against the real dots CLI on PATH and a disposable catalog. The
// extension receives the isolated environment only through its host `env`,
// so the follower and every one-shot call must read the same catalog.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdirSync, mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {setTimeout as sleep} from 'node:timers/promises';
import {systemClock} from '../../src/host/clock.ts';
import {harness} from '../wake-fixture.mjs';

/** Polls until `ready` holds, or fails after 10 seconds. */
async function until(what: string, ready: () => boolean): Promise<void> {
  const deadline = Date.now() + 10_000;
  while (!ready()) {
    assert.ok(Date.now() < deadline, `timed out waiting for ${what}`);
    await sleep(25);
  }
}

interface Catalog {
  readonly env: NodeJS.ProcessEnv;
  readonly project: string;
  /** Run `dots --json task ...` as another session; its parsed output. */
  task(...args: string[]): {task: {id: string}};
}

async function withCatalog(fn: (catalog: Catalog) => Promise<void>) {
  const dir = mkdtempSync(join(tmpdir(), 'dots-task-wakes-'));
  const env: NodeJS.ProcessEnv = {
    PATH: process.env.PATH,
    HOME: join(dir, 'home'),
    XDG_DATA_HOME: join(dir, 'data'),
    XDG_STATE_HOME: join(dir, 'state'),
  };
  try {
    mkdirSync(join(dir, 'home'));
    const checkout = join(dir, 'checkout');
    execFileSync('git', ['init', '--quiet', checkout], {env});
    const initialized = JSON.parse(
      execFileSync('dots', ['--json', 'project', 'init', 'fixture'], {
        cwd: checkout,
        env,
        encoding: 'utf8',
      }),
    ) as {project: {id: string}};
    const project = initialized.project.id;
    const task = (...args: string[]) =>
      JSON.parse(
        execFileSync(
          'dots',
          ['--json', 'task', ...args, '--project', project, '--owner', 'other'],
          {env, encoding: 'utf8'},
        ),
      ) as {task: {id: string}};
    await fn({env, project, task});
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
}

async function session(env: NodeJS.ProcessEnv) {
  const h = harness(systemClock, undefined, {env});
  await h.handlers.session_start({reason: 'startup'}, h.ctx);
  return h;
}

const taskWake = (
  h: Awaited<ReturnType<typeof session>>,
  project: string,
  id: string,
  until: string[],
) =>
  h.tools
    .get('wake')
    .execute(
      'c',
      {text: 'Integrate', task: {id, project}, until, limit: 5},
      undefined,
      undefined,
      h.ctx,
    ) as Promise<{details: {id: string}}>;

const contents = (h: Awaited<ReturnType<typeof session>>) =>
  h.messages.map((m: {message: {content: string}}) => m.message.content);

/** Fire message `index`, which must have arrived. */
const fired = (h: Awaited<ReturnType<typeof session>>, index: number) => {
  const content = contents(h)[index];
  assert.ok(content !== undefined, `fire message ${index}`);
  return content;
};

test(
  'a task wake follows its current subtree through the real store',
  {timeout: 60_000},
  () =>
    withCatalog(async ({env, project, task}) => {
      const a = task('add', 'Root A').task.id;
      const c = task('add', 'Root C').task.id;
      const b = task('add', 'Slice B', '--parent', a).task.id;
      const h = await session(env);
      try {
        // A wake on A sees B, then is cancelled; B then moves to C while
        // nothing follows either tree.
        const first = await taskWake(h, project, a, ['claimed']);
        task('claim', b);
        await until('the claim on B', () => contents(h).length === 1);
        assert.match(fired(h, 0), /claimed on descendant/);
        await h.call('wake_cancel', {id: first.details.id});
        task('release', b);
        task('update', b, '--parent', c);

        await taskWake(h, project, a, ['review', 'updated']);
        await taskWake(h, project, c, ['review', 'updated']);
        // Created after the wakes, below the moved slice.
        const d = task('add', 'Nested D', '--parent', b).task.id;
        task('review', d, '--result', 'Ready for review');
        await until('the review on D', () => contents(h).length === 2);
        assert.match(fired(h, 1), new RegExp(`task ${c} · matched`));
        assert.match(fired(h, 1), new RegExp(`review on descendant ${d}`));
        task('update', b, '--title', 'Slice B, renamed');
        await until('the update on B', () => contents(h).length === 3);
        assert.match(fired(h, 2), new RegExp(`task ${c} · matched`));
        await new Promise(resolve => setTimeout(resolve, 1_000));
        assert.equal(contents(h).length, 3, 'the wake on A never fired');
      } finally {
        await h.handlers.session_shutdown({}, h.ctx);
      }
    }),
);

test(
  'a restored task wake resumes from its cursor with no gap or repeat',
  {timeout: 60_000},
  () =>
    withCatalog(async ({env, project, task}) => {
      const a = task('add', 'Root A').task.id;
      const b = task('add', 'Slice B', '--parent', a).task.id;
      const h = await session(env);
      await taskWake(h, project, a, ['review', 'claimed']);
      task('claim', b);
      await until('the claim', () => contents(h).length === 1);
      await h.handlers.session_shutdown({}, h.ctx);

      // Written while Pi was away, then picked up on return.
      task('review', b, '--result', 'Ready for review');
      const resumed = harness(systemClock, undefined, {env});
      resumed.entries.push(...h.entries);
      await resumed.handlers.session_start({reason: 'resume'}, resumed.ctx);
      try {
        await until('the missed review', () => contents(resumed).length === 1);
        assert.match(fired(resumed, 0), /review on descendant/);
        assert.doesNotMatch(fired(resumed, 0), /#\d+ claimed/);
        await new Promise(resolve => setTimeout(resolve, 1_000));
        assert.equal(contents(resumed).length, 1);
      } finally {
        await resumed.handlers.session_shutdown({}, resumed.ctx);
      }
    }),
);
