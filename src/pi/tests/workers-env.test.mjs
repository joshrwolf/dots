import assert from 'node:assert/strict';
import test from 'node:test';
import {workerEnv} from '../extensions/workers/env.ts';

const worker = {id: 'w-1', depth: 1, root: 'w-1'};

test("a worker belongs to its parent's Rex terminal without owning it", () => {
  assert.deepEqual(workerEnv(worker, {REX_BLOCK: 'block:pane'}), {
    DOTS_WORKER_ID: 'w-1',
    DOTS_WORKER_DEPTH: '1',
    DOTS_WORKER_ROOT: 'w-1',
    DOTS_REX_BLOCK: 'block:pane',
    REX_BLOCK: undefined,
    PI_SESSION_ID: undefined,
    DOTS_AGENT_OWNER: undefined,
  });
});

test("a worker inherits neither its parent's session nor its runtime actor", () => {
  const env = workerEnv(worker, {
    PI_SESSION_ID: 'parent',
    DOTS_AGENT_OWNER: 'pi-parent-0123456789abcdef',
  });
  for (const name of ['PI_SESSION_ID', 'DOTS_AGENT_OWNER']) {
    assert.ok(name in env, name);
    assert.equal(env[name], undefined, name);
  }
});

test("a nested worker inherits the top session's terminal", () => {
  const env = workerEnv(
    {id: 'w-2', depth: 2, root: 'w-1'},
    {DOTS_REX_BLOCK: 'block:top'},
  );
  assert.equal(env.DOTS_REX_BLOCK, 'block:top');
  assert.ok('REX_BLOCK' in env && env.REX_BLOCK === undefined);
});

test('outside Rex a worker has no Rex terminal', () => {
  const env = workerEnv(worker, {REX_BLOCK: '', DOTS_REX_BLOCK: ''});
  assert.ok('DOTS_REX_BLOCK' in env);
  assert.equal(env.DOTS_REX_BLOCK, undefined);
});
