import assert from 'node:assert/strict';
import test from 'node:test';
import {workerEnv} from '../extensions/workers/env.ts';

const worker = {id: 'w-1', depth: 1, root: 'w-1'};

test('a worker belongs to its parent terminal Rex session without owning a terminal', () => {
  assert.deepEqual(
    workerEnv(worker, {REX_SESSION: 'session:top', REX_BLOCK: 'block:pane'}),
    {
      DOTS_WORKER_ID: 'w-1',
      DOTS_WORKER_DEPTH: '1',
      DOTS_WORKER_ROOT: 'w-1',
      DOTS_REX_SESSION: 'session:top',
      REX_SESSION: undefined,
      REX_BLOCK: undefined,
    },
  );
});

test('a nested worker inherits the top Rex session', () => {
  const env = workerEnv(
    {id: 'w-2', depth: 2, root: 'w-1'},
    {DOTS_REX_SESSION: 'session:top'},
  );
  assert.equal(env.DOTS_REX_SESSION, 'session:top');
  assert.ok('REX_SESSION' in env && env.REX_SESSION === undefined);
  assert.ok('REX_BLOCK' in env && env.REX_BLOCK === undefined);
});

test('outside Rex a worker has no Rex session', () => {
  const env = workerEnv(worker, {REX_SESSION: '', DOTS_REX_SESSION: ''});
  assert.ok('DOTS_REX_SESSION' in env);
  assert.equal(env.DOTS_REX_SESSION, undefined);
});
