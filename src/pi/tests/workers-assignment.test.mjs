import assert from 'node:assert/strict';
import test from 'node:test';
import {DotsError} from '@joshrwolf/pi-kit/dots';
import {
  decodeAssignment,
  verifyAssignment,
  assignmentContext,
} from '../extensions/workers/assignment.ts';

const assignment = {project: '/example/project', id: 't-123abc'};
const actor = 'pi-s-0123456789abcdef';
const shown = (id, fields = {}) => ({
  project: assignment.project,
  task: {id, status: 'open', owner: null, goal: null, holds: [], ...fields},
});
/** Answers `task show` from fixed records and logs every call. */
function store(records) {
  const calls = [];
  const client = async (args, options) => {
    calls.push([args, options]);
    const record = records[args[2]];
    if (!record) throw new Error(`no such task ${args[2]}`);
    return record;
  };
  return {calls, client};
}

test('assignment preflight checks exact scope without claims or checkout inference', async () => {
  const calls = [];
  const signal = new AbortController().signal;
  const result = await verifyAssignment(
    assignment,
    '/example/worktree',
    undefined,
    signal,
    async (...args) => {
      calls.push(args);
      return shown(assignment.id);
    },
  );
  assert.deepEqual(result, assignment);
  assert.deepEqual(calls, [
    [
      ['task', 'show', 't-123abc', '--project', '/example/project'],
      {directory: '/example/worktree', cwd: '/example/worktree', signal},
    ],
  ]);
  assert.match(assignmentContext(result), /parent owns task updates/);
  assert.equal(assignmentContext(undefined), '');
});

test('invalid and unavailable task references cannot masquerade as verified associations', async () => {
  for (const value of [
    null,
    {},
    {...assignment, project: ''},
    {...assignment, project: 'a\nb'},
    {...assignment, id: 'unknown'},
    {...assignment, extra: true},
  ])
    assert.throws(() => decodeAssignment(value), /Task assignment/);
  for (const response of [
    null,
    {},
    {...shown(assignment.id), project: '/other'},
    shown('t-abcdef'),
  ]) {
    await assert.rejects(
      verifyAssignment(
        assignment,
        '/work',
        actor,
        undefined,
        async () => response,
      ),
      /different project or task/,
    );
  }
  // A CLI that cannot say whether the task belongs to a goal cannot verify it.
  for (const task of [
    {id: assignment.id},
    {...shown(assignment.id).task, goal: 'unknown'},
    {...shown(assignment.id).task, holds: null},
  ])
    await assert.rejects(
      verifyAssignment(assignment, '/work', actor, undefined, async () => ({
        project: assignment.project,
        task,
      })),
      /lacks goal association or holds/,
    );
  const failure = new DotsError(
    ['task', 'show'],
    'failed',
    'no such task',
    'no such task',
  );
  await assert.rejects(
    verifyAssignment(assignment, '/work', actor, undefined, async () => {
      throw failure;
    }),
    error => {
      assert.match(error.message, /^Task lookup failed: no such task/);
      assert.equal(error.cause, failure, 'the dots failure stays inspectable');
      return true;
    },
  );
});

test('only the goal coordinator dispatches goal work, fresh from the store', async () => {
  const root = 't-a0a0a0';
  const records = {
    [root]: shown(root, {goal: root, owner: actor}),
    [assignment.id]: shown(assignment.id, {goal: root}),
  };
  const {calls, client} = store(records);
  assert.deepEqual(
    await verifyAssignment(assignment, '/work', actor, undefined, client),
    assignment,
  );
  assert.deepEqual(
    calls.map(([args]) => args[2]),
    [assignment.id, root],
  );
  // The root itself, with unfinished children, is coordinator responsibility.
  const whole = {...assignment, id: root};
  records[root].task.children = 3;
  records[root].task.unclosed_children = 3;
  assert.deepEqual(
    await verifyAssignment(whole, '/work', actor, undefined, client),
    whole,
  );

  for (const [label, runtime, owner] of [
    ['another runtime of the session', 'pi-s-fedcba9876543210', actor],
    ['an unbound runtime', undefined, actor],
    ['an uncoordinated goal', actor, null],
  ]) {
    records[root].task.owner = owner;
    await assert.rejects(
      verifyAssignment(assignment, '/work', runtime, undefined, client),
      /only its coordinator dispatches goal work/,
      label,
    );
  }
});

test('held goal work is not dispatched; other work is', async () => {
  const root = 't-a0a0a0';
  const records = {
    [root]: shown(root, {goal: root, owner: actor}),
    [assignment.id]: shown(assignment.id, {goal: root}),
  };
  const {client} = store(records);
  const verify = () =>
    verifyAssignment(assignment, '/work', actor, undefined, client);
  records[assignment.id].task.status = 'held';
  await assert.rejects(verify(), /t-123abc is held; resume it/);
  records[assignment.id].task.status = 'open';
  records[assignment.id].task.holds = [{kind: 'held_ancestor', task: root}];
  await assert.rejects(verify(), /held ancestor t-a0a0a0/);
  // A goal-wide blocker seen on the fresher root read counts as well.
  records[assignment.id].task.holds = [];
  records[root].task.status = 'held';
  await assert.rejects(verify(), /t-a0a0a0 is held; resume it/);
  records[root].task.status = 'open';
  records[root].task.holds = [{kind: 'held_ancestor', task: 't-b0b0b0'}];
  await assert.rejects(verify(), /t-a0a0a0 is held by held ancestor t-b0b0b0/);
  records[root].task.holds = [];
  assert.deepEqual(await verify(), assignment);
  // A task outside any goal keeps the plain identity check.
  records[assignment.id] = shown(assignment.id, {status: 'held'});
  assert.deepEqual(await verify(), assignment);
});
