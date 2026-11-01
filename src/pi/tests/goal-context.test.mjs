import assert from 'node:assert/strict';
import test from 'node:test';
import {
  briefContext,
  taskContext,
  goalSections,
  goalSectionKey,
} from '../extensions/goal/context.ts';
import {snapshot, task} from './goal-fixture.mjs';

function fixture() {
  const value = snapshot();
  value.overview.goal.briefContent.revision = 'sha256:' + 'a'.repeat(64);
  return {focus: {id: 't-123abc', project: value.project}, snapshot: value};
}

test('automatic goal sections ignore clocks, heartbeats, worker history and read order', () => {
  const view = fixture();
  view.snapshot.overview.tasks.push(task('t-aaaaaa', 'Second slice'));
  const before = goalSections(view);
  view.snapshot.now += 61;
  view.snapshot.overview.tasks.reverse();
  view.snapshot.workers = {
    rows: Array.from({length: 100}, (_, i) => ({
      id: `w-${i}`,
      state: i ? 'completed' : 'running',
      label: 'Worker',
      updated_at: Date.now(),
      session_file: '/example/worker.jsonl',
    })),
    truncated: true,
  };
  assert.deepEqual(goalSections(view), before);
  assert.doesNotMatch(
    Object.values(before).join('\n'),
    /Recorded at|Worker|jsonl/,
  );
});

test('brief, task and project identities update independent native sections', () => {
  const view = fixture();
  const initial = goalSections(view);
  const key = goalSectionKey(view);
  view.snapshot.overview.tasks[0].status = 'review';
  const changed = goalSections(view);
  assert.equal(changed[`${key}_brief`], initial[`${key}_brief`]);
  assert.notEqual(changed[`${key}_tasks`], initial[`${key}_tasks`]);
  view.snapshot.overview.goal.briefContent.text = '# Revised agreement';
  view.snapshot.overview.goal.briefContent.revision =
    'sha256:' + 'b'.repeat(64);
  const revised = goalSections(view);
  assert.equal(revised[`${key}_tasks`], changed[`${key}_tasks`]);
  assert.notEqual(revised[`${key}_brief`], changed[`${key}_brief`]);
  assert.notEqual(
    goalSectionKey({...view, focus: {...view.focus, project: '/other'}}),
    key,
  );
  assert.match(key, /^[a-z][a-z0-9_]+$/);
});

test('task state shows claims, attention, ancestry and result revisions without result text', () => {
  const view = fixture();
  view.snapshot.overview.tasks = [
    task('t-aaaaaa', 'Claimed', {
      owner: 'coordinator',
      branch: 'feature',
      parent: 't-group',
      result_event: 7,
      result: 'LONG_RESULT',
      last_event: {
        seq: 9,
        kind: 'updated',
        at: 1,
        actor: 'owner',
        detail: 'LONG_EVENT',
      },
    }),
    task('t-bbbbbb', 'Waiting', {status: 'held', blocked_by: ['t-aaaaaa']}),
  ];
  const before = taskContext(view);
  assert.ok(before.indexOf('Waiting') < before.indexOf('Claimed'));
  assert.match(
    before,
    /parent t-group; claimed by coordinator; branch feature; result event 7/,
  );
  assert.match(before, /blocked by t-aaaaaa/);
  assert.doesNotMatch(before, /LONG_RESULT|LONG_EVENT/);
  view.snapshot.now = 101;
  assert.equal(taskContext(view), before, 'claims do not expire');
  const claimed = view.snapshot.overview.tasks[0];
  claimed.result = 'REVISED';
  claimed.result_event = 12;
  const revised = taskContext(view);
  assert.notEqual(revised, before, 'a revised result is visible');
  assert.match(revised, /result event 12/);
  assert.doesNotMatch(revised, /REVISED/);
  view.snapshot.overview.goal.root.result_event = 13;
  assert.match(taskContext(view), /\[open\] · result event 13/);
});

test('large inputs stay bounded and explicit about omissions; agreements are never silently summarized', () => {
  const view = fixture();
  const page =
    '# Agreement\n' +
    'Agreement text. '.repeat(700) +
    '👩'.repeat(300) +
    '\nWorking agreement at the end of the bounded CLI page.';
  assert.ok(Buffer.byteLength(page) < 16 * 1024);
  view.snapshot.overview.goal.briefContent.text = page;
  view.snapshot.overview.tasks = Array.from({length: 200}, (_, i) =>
    task(`t-${i}`, 'Slice '.repeat(100), {
      result: 'x'.repeat(10_000),
      references: [],
      branch: 'branch'.repeat(100),
    }),
  );
  const brief = briefContext(view);
  const tasks = taskContext(view);
  assert.ok(
    brief.includes(page),
    'do not cut the already bounded source page again',
  );
  assert.doesNotMatch(brief, /Brief preview limited/);
  assert.doesNotMatch(brief, /[\uD800-\uDBFF](?![\uDC00-\uDFFF])/u);
  assert.ok(brief.length < page.length + 1000);
  assert.ok(tasks.length < 9500);
  assert.match(tasks, /more unfinished tasks not shown/);
  const actualRows = tasks
    .split('\n')
    .filter(line => /^t-\d+ \[/.test(line)).length;
  assert.ok(actualRows <= 40);
  assert.match(
    tasks,
    new RegExp(`${200 - actualRows} more unfinished tasks not shown`),
  );
});

test('claim changes are material while event bookkeeping stays stable', () => {
  const view = fixture();
  const root = view.snapshot.overview.goal.root;
  root.owner = 'root-owner';
  root.last_event = {seq: 1, kind: 'claimed'};
  view.snapshot.overview.tasks[0].owner = 'slice-owner';
  const initial = goalSections(view);
  assert.match(taskContext(view), /\[open\] · claimed by root-owner/);
  assert.match(taskContext(view), /claimed by slice-owner/);
  root.last_event = {seq: 3, kind: 'updated'};
  view.snapshot.now = 150;
  assert.deepEqual(goalSections(view), initial);
  root.owner = null;
  view.snapshot.overview.tasks[0].owner = null;
  assert.doesNotMatch(taskContext(view), /claimed by/);
  assert.notDeepEqual(goalSections(view), initial);
});

test('truncated source pages retain the native warning and complete-record route', () => {
  const view = fixture();
  view.snapshot.overview.goal.briefContent.warning =
    'Brief truncated; read /example/brief.md from line 201 for the rest.';
  assert.match(briefContext(view), /from line 201/);
  assert.match(
    briefContext(view),
    /Read the full brief at \/example\/brief.md before acting/,
  );
});

test('unavailable observations replace context; no unfinished slices is not goal acceptance', () => {
  const view = fixture();
  view.snapshot.overview.tasks = [];
  assert.match(taskContext(view), /\[open\]/);
  assert.match(taskContext(view), /No unfinished tasks\./);
  assert.doesNotMatch(taskContext(view), /No execution|Goal complete/);
  const unavailable = {...view, error: 'read failed'};
  assert.match(briefContext(unavailable), /Brief unavailable/);
  assert.match(taskContext(unavailable), /Task state unavailable/);
  assert.doesNotMatch(
    Object.values(goalSections(unavailable)).join('\n'),
    /Use one shared export interface|Reference slice/,
  );
});

test("the goal's own result is its bounded running handoff", () => {
  const view = fixture();
  const root = view.snapshot.overview.goal.root;
  assert.doesNotMatch(taskContext(view), /Current state:/);
  root.result = 'Tests pass on main.\nNext: write the README.';
  root.result_event = 4;
  assert.match(
    taskContext(view),
    /Current state:\nTests pass on main\.\nNext: write the README\./,
  );
  root.result = 'x'.repeat(5000);
  const bounded = taskContext(view);
  assert.match(
    bounded,
    /…\n\[Truncated; dots task --project .* show t-123abc\]/,
  );
  assert.ok(bounded.length < 4000);
});
