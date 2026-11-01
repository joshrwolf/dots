import assert from 'node:assert/strict';
import {execFile, execFileSync} from 'node:child_process';
import {mkdtempSync, mkdirSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {createCheckoutResolver} from '../extensions/workers/checkout.ts';

const git = (cwd, ...args) =>
  execFileSync('git', args, {
    cwd,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
const run = (command, args, options) =>
  new Promise(resolve => {
    execFile(
      command,
      args,
      {
        cwd: options.cwd,
        timeout: options.timeout,
        signal: options.signal,
        encoding: 'utf8',
      },
      (error, stdout, stderr) =>
        resolve({
          code: error ? (typeof error.code === 'number' ? error.code : -1) : 0,
          killed: Boolean(error?.killed),
          stdout,
          stderr,
        }),
    );
  });
async function until(check) {
  for (let i = 0; i < 150; i++) {
    const value = check();
    if (value) return value;
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  throw new Error('checkout observation did not settle');
}

test('checkout cues verify branch, detached HEAD and non-Git folders; same checkout is omitted', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dots-checkout-'));
  const a = path.join(root, 'main');
  const b = path.join(root, 'feature');
  const detached = path.join(root, 'detached');
  const folder = path.join(root, 'plain');
  let calls = 0;
  let changes = 0;
  const resolver = createCheckoutResolver(
    (...args) => {
      calls++;
      return run(...args);
    },
    () => {
      changes++;
    },
  );
  try {
    for (const [cwd, branch] of [
      [a, 'main'],
      [b, 'feature/review'],
      [detached, 'main'],
    ]) {
      mkdirSync(cwd);
      git(cwd, 'init', '--quiet', '--initial-branch', branch);
      git(
        cwd,
        '-c',
        'user.name=Fixture',
        '-c',
        'user.email=fixture@example.invalid',
        '-c',
        'commit.gpgsign=false',
        'commit',
        '--quiet',
        '--allow-empty',
        '-m',
        'fixture',
      );
    }
    git(detached, 'checkout', '--quiet', '--detach', 'HEAD');
    mkdirSync(folder);
    const nested = path.join(a, 'nested');
    mkdirSync(nested);
    assert.deepEqual(await until(() => resolver.get(b, a)), {
      kind: 'branch',
      label: 'feature/review',
    });
    assert.deepEqual(await until(() => resolver.get(detached, a)), {
      kind: 'branch',
      label: git(detached, 'rev-parse', '--short=8', 'HEAD'),
      detached: true,
    });
    assert.deepEqual(await until(() => resolver.get(folder, a)), {
      kind: 'directory',
      label: 'plain',
    });
    resolver.get(nested, a);
    await until(() => changes >= 5); // Each distinct directory is observed outside rendering.
    assert.equal(
      resolver.get(nested, a),
      undefined,
      'a subdirectory of the same checkout is not another checkout',
    );
    assert.deepEqual(
      resolver.branch(a),
      {kind: 'branch', label: 'main'},
      "inspector can show verified Git identity even for the owner's checkout",
    );
    const checked = calls;
    resolver.get(b, a);
    resolver.get(detached, a);
    assert.equal(calls, checked, 'fresh observations are cached by cwd');
  } finally {
    resolver.dispose();
    rmSync(root, {recursive: true, force: true});
  }
});

test('timeouts are unknown, never a non-Git folder; disposal prevents late observations', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dots-checkout-timeout-'));
  let changes = 0;
  const slow = createCheckoutResolver(
    async () => ({code: -1, killed: true, stdout: '', stderr: 'timed out'}),
    () => {
      changes++;
    },
  );
  try {
    assert.equal(slow.get(path.join(root, 'worker'), root), undefined);
    await until(() => changes === 2);
    assert.equal(slow.get(path.join(root, 'worker'), root), undefined);
    let release;
    const delayed = createCheckoutResolver(
      () =>
        new Promise(resolve => {
          release = resolve;
        }),
      () => {
        changes++;
      },
    );
    delayed.get(path.join(root, 'later'), root);
    const previous = changes;
    delayed.dispose();
    release({
      code: 128,
      killed: false,
      stdout: '',
      stderr: 'fatal: not a git repository',
    });
    await new Promise(resolve => setTimeout(resolve, 20));
    assert.equal(
      changes,
      previous,
      'disposed observer cannot request a stale UI redraw',
    );
  } finally {
    slow.dispose();
    rmSync(root, {recursive: true, force: true});
  }
});
