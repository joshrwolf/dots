import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {DotsError, dots} from '../src/dots.ts';

function isolated(t: test.TestContext) {
  const dir = mkdtempSync(path.join(tmpdir(), 'pi-kit-dots-'));
  t.after(() => rmSync(dir, {recursive: true, force: true}));
  return {
    dir,
    env: {
      ...process.env,
      HOME: dir,
      XDG_STATE_HOME: path.join(dir, 'state'),
      XDG_DATA_HOME: path.join(dir, 'data'),
    },
  };
}

test('real dots honors JSON, explicit project and isolated store/directory', async t => {
  const {dir, env} = isolated(t);
  execFileSync('git', ['init', '--quiet', dir], {env});
  const initialized = await dots(['project', 'init', 'Fixture', '--fresh'], {
    env,
    directory: dir,
    cwd: dir,
  });
  assert.ok(
    initialized && typeof initialized === 'object' && 'project' in initialized,
  );
  const project = initialized.project;
  assert.ok(
    project &&
      typeof project === 'object' &&
      'id' in project &&
      typeof project.id === 'string',
  );
  const result = await dots(['task', 'list', '--project', project.id], {
    env,
    directory: dir,
    cwd: dir,
  });
  assert.ok(
    result &&
      typeof result === 'object' &&
      'project' in result &&
      'tasks' in result,
  );
  assert.equal(result.project, project.id);
  assert.deepEqual(result.tasks, []);
});

test('failed real dots run preserves stderr and native process cause', async t => {
  const {dir, env} = isolated(t);
  await assert.rejects(
    dots(['task', 'show', 't-000000', '--project', 'fixture-project'], {
      env,
      cwd: dir,
    }),
    (error: unknown) => {
      assert.ok(error instanceof DotsError);
      assert.equal(error.reason, 'failed');
      assert.equal(error.args[1], 'show');
      assert.ok(error.stderr.length > 0);
      assert.ok(error.cause instanceof Error);
      return true;
    },
  );
});

test('real non-JSON CLI response preserves the parse failure separately', async t => {
  const {dir, env} = isolated(t);
  await assert.rejects(
    dots(['--help'], {env, cwd: dir}),
    (error: unknown) =>
      error instanceof DotsError &&
      error.reason === 'invalid-json' &&
      error.cause instanceof SyntaxError,
  );
});

test('missing executable and pre-aborted calls do not reach another store', async t => {
  const {dir, env} = isolated(t);
  const unavailable = {...env, PATH: dir};
  await assert.rejects(
    dots(['memory', 'context'], {env: unavailable, cwd: dir}),
    (error: unknown) =>
      error instanceof DotsError &&
      error.cause instanceof Error &&
      'code' in error.cause &&
      error.cause.code === 'ENOENT',
  );
  const cause = new Error('cancelled before spawn');
  await assert.rejects(
    dots(['memory', 'context'], {
      env: unavailable,
      cwd: dir,
      signal: AbortSignal.abort(cause),
    }),
    (error: unknown) => error === cause,
  );
});

test('DotsError retains caller-supplied native cause', () => {
  const cause = new SyntaxError('bad JSON');
  const error = new DotsError(
    ['memory', 'context'],
    'invalid-json',
    '',
    'unavailable',
    {cause},
  );
  assert.equal(error.cause, cause);
  assert.equal(error.name, 'DotsError');
});
