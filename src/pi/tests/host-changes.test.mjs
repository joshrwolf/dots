import assert from 'node:assert/strict';
import {mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {watchChanges} from '../src/host/changes.ts';

test('touching the changes marker notifies; closed watches do not', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'dots-changes-'));
  try {
    const marker = join(dir, 'changes');
    let calls = 0;
    const watcher = watchChanges(marker, () => calls++);
    assert.ok(watcher);
    // The OS may drop writes made just after a watch starts; keep touching
    // the marker until one is seen, within a bounded deadline.
    const deadline = Date.now() + 30_000;
    while (calls === 0 && Date.now() < deadline) {
      writeFileSync(marker, String(Date.now()));
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    assert.ok(calls > 0, 'no change seen within 30 s');
    watcher.close();
    // After close, nothing more is delivered, however late the OS reports.
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
