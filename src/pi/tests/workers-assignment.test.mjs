import assert from 'node:assert/strict';
import test from 'node:test';
import {DotsError} from '@joshrwolf/pi-kit/dots';
import {
  decodeAssignment,
  verifyAssignment,
  assignmentContext,
} from '../extensions/workers/assignment.ts';

const assignment = {project: '/example/project', id: 't-123abc'};
test('assignment preflight checks exact scope without claims or checkout inference', async () => {
  const calls = [];
  const signal = new AbortController().signal;
  const result = await verifyAssignment(
    assignment,
    '/example/worktree',
    signal,
    async (...args) => {
      calls.push(args);
      return {project: assignment.project, task: {id: assignment.id}};
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
    {project: '/other', task: {id: assignment.id}},
    {project: assignment.project, task: {id: 't-abcdef'}},
  ]) {
    await assert.rejects(
      verifyAssignment(assignment, '/work', undefined, async () => response),
      /different project or task/,
    );
  }
  const failure = new DotsError(
    ['task', 'show'],
    'failed',
    'no such task',
    'no such task',
  );
  await assert.rejects(
    verifyAssignment(assignment, '/work', undefined, async () => {
      throw failure;
    }),
    error => {
      assert.match(error.message, /^Task lookup failed: no such task/);
      assert.equal(error.cause, failure, 'the dots failure stays inspectable');
      return true;
    },
  );
});
