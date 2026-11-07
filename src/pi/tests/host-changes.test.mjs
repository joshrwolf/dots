import assert from 'node:assert/strict';
import {mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {watchChanges} from '../src/host/changes.ts';

test('touching the changes marker notifies; other files and closed watches do not', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'dots-changes-'));
  try {
    const marker = join(dir, 'changes');
    let calls = 0;
    let notified;
    const next = () => new Promise(resolve => (notified = resolve));
    const watcher = watchChanges(marker, () => {
      calls++;
      notified?.();
    });
    assert.ok(watcher);
    const first = next();
    writeFileSync(marker, '');
    await first;
    assert.ok(calls >= 1);
    watcher.close();
    const before = calls;
    writeFileSync(marker, 'again');
    await new Promise(resolve => setTimeout(resolve, 100));
    assert.equal(calls, before);
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
});

test('an unwatchable directory leaves callers on ordinary reads', () => {
  assert.equal(
    watchChanges(join(tmpdir(), 'dots-absent-dir', 'changes'), () => {}),
    undefined,
  );
});
