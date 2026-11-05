import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, writeFileSync, rmSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {
  decodeList,
  decodeSnapshot as decodeCliSnapshot,
  dotsSource,
  withHierarchy,
} from '../extensions/goal/source.ts';
import {snapshot} from './goal-fixture.mjs';
import {goalSections} from '../extensions/goal/context.ts';

// Build the wire contract independently from the companion's presentation model.
function cliResponse(value) {
  const goal = value?.overview?.goal;
  if (!goal) return value;
  const {briefContent, ...rest} = goal;
  const {progress, ...overview} = value.overview;
  return {
    ...value,
    overview: {
      ...overview,
      goal: {
        ...rest,
        progress,
        last_activity: value.now,
        brief: {
          relative: 'briefs/brief.md',
          path: goal.brief,
          diagnostic: null,
          document: {
            path: goal.brief,
            revision: 'sha256:' + 'a'.repeat(64),
            exists: true,
            content: briefContent.text,
            start: 1,
            total_lines: 4,
            next_line: null,
          },
        },
      },
    },
  };
}
function decodeSnapshot(value) {
  const result = decodeCliSnapshot(cliResponse(value));
  delete result.overview.goal.briefContent.revision;
  return result;
}

test('CLI responses are validated rather than coerced into success', () => {
  assert.deepEqual(decodeSnapshot(snapshot()), snapshot());
  assert.equal(decodeList({project: 'example', goals: []}).goals.length, 0);
  for (const value of [
    null,
    {},
    {error: 'unsupported'},
    {...snapshot(), overview: {}},
  ])
    assert.throws(() => decodeSnapshot(value));
  for (const changed of ['status', 'progress', 'brief']) {
    const value = snapshot();
    if (changed === 'status') value.overview.goal.root.status = 'unknown';
    if (changed === 'progress') value.overview.progress.done = -1;
    if (changed === 'brief') value.overview.goal.brief = 'relative.md';
    assert.throws(() => decodeSnapshot(value));
  }
});

test('review results and references decode strictly', () => {
  const value = snapshot();
  const item = value.overview.tasks[0];
  item.status = 'review';
  item.result = 'Tests pass; acceptance pending.';
  item.references = [
    {
      url: 'https://linear.app/example/issue/ENG-123',
      label: 'ENG-123',
    },
  ];
  value.overview.progress = {
    ...value.overview.progress,
    open: 0,
    review: 1,
    ready: 0,
  };
  assert.deepEqual(decodeSnapshot(value), value);
  for (const fields of [{url: 7}, {label: 3}]) {
    const bad = structuredClone(value);
    Object.assign(bad.overview.tasks[0].references[0], fields);
    assert.throws(() => decodeSnapshot(bad));
  }
  delete item.result;
  assert.throws(() => decodeSnapshot(value));
});

test('real isolated source reads complete history and selects the explicit project', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'dots-goal-source-'));
  const keys = [
    'HOME',
    'XDG_STATE_HOME',
    'XDG_DATA_HOME',
    'PI_CODING_AGENT_DIR',
  ];
  const previous = keys.map(key => process.env[key]);
  for (const key of keys) process.env[key] = join(dir, key);
  let project;
  let owner = 'fixture';
  // The fixture acts for itself, not for an agent that launched the tests.
  const env = () =>
    Object.fromEntries(
      Object.entries(process.env).filter(
        ([key]) =>
          !['DOTS_WORKER_ID', 'DOTS_AGENT_OWNER', 'PI_SESSION_ID'].includes(
            key,
          ),
      ),
    );
  const cli = (...args) =>
    JSON.parse(
      execFileSync(
        'dots',
        [
          '--json',
          '--directory',
          dir,
          'task',
          ...args,
          '--project',
          project,
          '--owner',
          owner,
        ],
        {encoding: 'utf8', env: env()},
      ),
    );
  try {
    mkdirSync(process.env.HOME, {recursive: true});
    execFileSync('git', ['init', '--quiet', dir]);
    project = JSON.parse(
      execFileSync(
        'dots',
        ['--json', '--directory', dir, 'project', 'init', 'Fixture', '--fresh'],
        {encoding: 'utf8'},
      ),
    ).project.id;
    const brief = join(
      process.env.XDG_DATA_HOME,
      'dots/workspaces',
      project,
      'briefs/brief.md',
    );
    mkdirSync(join(brief, '..'), {recursive: true});
    writeFileSync(brief, '# Fixture goal');
    const created = cli(
      'goal',
      'create',
      'Fixture',
      '--brief',
      'briefs/brief.md',
    );
    const id = created.goal.root.id;
    // Only the goal's coordinator changes its tasks.
    cli('goal', 'coordinate', id);
    const open = cli('add', 'Open slice', '--parent', id);
    const closed = cli('add', 'Closed slice', '--parent', id);
    cli('done', closed.task.id, '--result', 'Fixture verified');
    const result = await dotsSource.show(
      dir,
      id,
      new AbortController().signal,
      false,
      project,
    );
    assert.equal(result.project, project);
    assert.equal(result.overview.goal.brief, brief);
    assert.equal(result.overview.goal.briefContent.text, '# Fixture goal\n');
    assert.equal(result.overview.tasks.length, 1);
    assert.equal(result.overview.hidden_closed, 1);
    assert.equal(result.allTasks.length, 2);
    const history = await dotsSource.show(
      dir,
      id,
      new AbortController().signal,
      true,
      project,
    );
    assert.equal(history.overview.tasks.length, 2);
    assert.equal(history.overview.hidden_closed, 0);
    assert.deepEqual(
      withHierarchy(result, true).overview.tasks,
      history.overview.tasks,
    );
    cli('claim', open.task.id);
    const claimed = await dotsSource.show(
      dir,
      id,
      new AbortController().signal,
      false,
      project,
    );
    assert.equal(claimed.overview.tasks[0].owner, 'fixture');
    assert.match(
      Object.values(
        goalSections({focus: {id, project}, snapshot: claimed}),
      ).join('\n'),
      /claimed by fixture/,
    );
    cli('release', open.task.id);
    // Release acts as the captured runtime only, never a later runtime of the
    // same session.
    const ended = {session: 'fixture', owner: 'pi-fixture-0123456789abcdef'};
    const current = {session: 'fixture', owner: 'pi-fixture-fedcba9876543210'};
    // A goal has one coordinator, so each runtime coordinates its own goal.
    cli('release', id);
    owner = ended.owner;
    cli('goal', 'coordinate', id);
    cli('claim', open.task.id);
    owner = current.owner;
    const other = cli('goal', 'create', 'Other', '--brief', 'briefs/brief.md')
      .goal.root.id;
    cli('goal', 'coordinate', other);
    const second = cli('add', 'Second slice', '--parent', other);
    cli('claim', second.task.id);
    await dotsSource.release(ended);
    const released = await dotsSource.show(
      dir,
      id,
      new AbortController().signal,
      false,
      project,
    );
    assert.deepEqual(
      Object.fromEntries(
        released.overview.tasks.map(task => [task.id, task.owner]),
      ),
      {[open.task.id]: null},
    );
    assert.equal(cli('show', second.task.id).task.owner, current.owner);
    await assert.rejects(
      dotsSource.show(
        dir,
        id,
        new AbortController().signal,
        false,
        join(dir, 'other'),
      ),
      error =>
        error.cause?.name === 'DotsError' && error.cause.cause instanceof Error,
    );
    const partial = snapshot();
    partial.overview.hidden_closed = 1;
    assert.equal(withHierarchy(partial), partial);
  } finally {
    keys.forEach((key, i) => {
      if (previous[i] === undefined) delete process.env[key];
      else process.env[key] = previous[i];
    });
    rmSync(dir, {recursive: true, force: true});
  }
});

test('brief content, truncation and failures come from the CLI snapshot only', () => {
  const value = cliResponse(snapshot());
  const brief = value.overview.goal.brief;
  brief.document.next_line = 201;
  assert.match(
    decodeCliSnapshot(value).overview.goal.briefContent.warning,
    /truncated.*201/,
  );
  brief.document = null;
  brief.diagnostic = 'symlink document path refused';
  const unavailable = decodeCliSnapshot(value).overview.goal.briefContent;
  assert.equal(unavailable.text, '');
  assert.match(unavailable.warning, /unavailable.*symlink/);
  for (const fields of [
    {exists: 'true'},
    {revision: 'unknown'},
    {path: '/different.md'},
    {next_line: 0},
    {start: 2},
    {content: null},
  ]) {
    const invalid = cliResponse(snapshot());
    Object.assign(invalid.overview.goal.brief.document, fields);
    assert.throws(() => decodeCliSnapshot(invalid));
  }
  const missing = cliResponse(snapshot());
  Object.assign(missing.overview.goal.brief.document, {
    exists: false,
    revision: 'missing',
    content: '',
    total_lines: 0,
  });
  assert.match(
    decodeCliSnapshot(missing).overview.goal.briefContent.warning,
    /missing/,
  );
});

test("goal timestamps reject safe integers outside Date's rendering domain", () => {
  for (const value of [-1, 0.5, Number.MAX_SAFE_INTEGER, 8_640_000_000_001]) {
    const now = snapshot();
    now.now = value;
    assert.throws(() => decodeSnapshot(now), /timestamp/);
    const event = snapshot();
    event.overview.goal.root.last_event = {
      seq: 1,
      at: value,
      kind: 'note',
      actor: null,
      detail: null,
      task: event.overview.goal.root.id,
    };
    assert.throws(() => decodeSnapshot(event), /timestamp/);
  }
  for (const value of [0, 100, 8_640_000_000_000]) {
    const now = snapshot();
    now.now = value;
    assert.equal(decodeSnapshot(now).now, value);
  }
});

test('latest task events decode strictly and tolerate absent or null records', () => {
  const value = snapshot();
  const item = value.overview.tasks[0];
  item.status = 'held';
  item.last_event = {
    seq: 7,
    at: 90,
    kind: 'updated',
    actor: null,
    detail: 'Implemented; committed locally.',
    task: item.id,
  };
  assert.deepEqual(
    decodeSnapshot(value).overview.tasks[0].last_event,
    item.last_event,
  );
  item.last_event = null;
  assert.equal(decodeSnapshot(value).overview.tasks[0].last_event, null);
  delete item.last_event;
  assert.equal(decodeSnapshot(value).overview.tasks[0].last_event, null);
  const good = {
    seq: 1,
    at: 90,
    kind: 'hold',
    actor: 'fixture',
    detail: null,
    task: item.id,
  };
  for (const bad of [
    'note',
    [],
    {...good, at: -1},
    {...good, kind: 3},
    {...good, seq: '1'},
    {...good, actor: undefined},
  ]) {
    item.last_event = bad;
    assert.throws(() => decodeSnapshot(value));
  }
});
