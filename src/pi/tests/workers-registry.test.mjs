import assert from 'node:assert/strict';
import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {openRegistry, readTaskWorkers} from '../extensions/workers/registry.ts';

const row = (id, parent, root, state, extra = {}) => ({
  id,
  parent,
  root,
  owner_session: null,
  label: id,
  state,
  cwd: '/w',
  task_project: null,
  task_id: null,
  model: null,
  thinking: null,
  activity: null,
  failed: null,
  question: null,
  calls: 0,
  tokens: 0,
  started_at: 1000,
  last_event_at: 1000,
  updated_at: 1000,
  session_file: null,
  pid: 1,
  ...extra,
});

test('worker model and effort observations survive registry reopen and updates', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'dots-registry-model-'));
  const file = path.join(dir, 'workers.db');
  let registry = openRegistry(file, m => assert.fail(m));
  try {
    registry.upsert(
      row('child', 'parent', 'root', 'running', {
        model: 'example/reasoning-model',
        thinking: 'high',
      }),
    );
    registry.close();
    registry = openRegistry(file, m => assert.fail(m));
    const [child] = registry.children('parent');
    assert.equal(child.model, 'example/reasoning-model');
    assert.equal(child.thinking, 'high');
    registry.upsert({...child, thinking: 'medium'});
    assert.equal(registry.children('parent')[0].thinking, 'medium');
  } finally {
    registry?.close();
    rmSync(dir, {recursive: true, force: true});
  }
});

test('context observations persist separately from activity freshness and preserve unknown usage', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'dots-registry-context-'));
  const file = path.join(dir, 'workers.db');
  let registry = openRegistry(file, m => assert.fail(m));
  try {
    registry.upsert(row('child', 'parent', 'root', 'running'));
    assert.equal(registry.context('child'), undefined);
    const snapshot = {
      model: 'anthropic/fixture',
      tokens: 250_000,
      window: 1_000_000,
      compactAt: 300_000,
      compactions: 2,
    };
    registry.recordContext('child', snapshot, 1000);
    registry.upsert(
      row('child', 'parent', 'root', 'running', {updated_at: 9000}),
    );
    assert.deepEqual(registry.context('child'), {snapshot, observedAt: 1000});
    registry.close();
    registry = openRegistry(file, m => assert.fail(m));
    assert.deepEqual(registry.context('child'), {snapshot, observedAt: 1000});
    registry.recordContext(
      'child',
      {...snapshot, tokens: null, compactAt: null, compactions: 3},
      10000,
    );
    assert.equal(registry.context('child').snapshot.compactions, 3);
    assert.equal(registry.context('child').snapshot.tokens, null);
    assert.equal(registry.context('child').snapshot.compactAt, null);
    assert.equal(registry.context('child').observedAt, 10000);
  } finally {
    registry?.close();
    rmSync(dir, {recursive: true, force: true});
  }
});

test('registry: tree summary counts descendants at any depth by state', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'dots-registry-'));
  try {
    const reg = openRegistry(path.join(dir, 'nested/workers.db'), m =>
      assert.fail(m),
    );
    assert.ok(reg);
    reg.upsert(row('a', null, 'a', 'running'));
    reg.upsert(row('a1', 'a', 'a', 'running', {last_event_at: 0}));
    reg.upsert(row('a2', 'a', 'a', 'asking', {question: 'which?'}));
    reg.upsert(
      row('a2x', 'a2', 'a', 'running', {failed: 'bash', last_event_at: 95_000}),
    );
    reg.upsert(row('a3', 'a', 'a', 'completed'));
    reg.upsert(row('b', null, 'b', 'running'));

    assert.deepEqual(reg.subtree('a', 100_000, 90_000), {
      live: 3,
      asking: 1,
      failed: 1,
      quiet: 1,
    });
    assert.deepEqual(reg.subtree('a2', 100_000, 90_000), {
      live: 1,
      asking: 0,
      failed: 1,
      quiet: 0,
    });
    assert.deepEqual(reg.subtree('b', 100_000, 90_000), {
      live: 0,
      asking: 0,
      failed: 0,
      quiet: 0,
    });
    assert.deepEqual(
      reg.children(null).map(r => r.id),
      ['a', 'b'],
    );

    reg.upsert(row('a1', 'a', 'a', 'completed'));
    assert.equal(
      reg.subtree('a', 100_000, 90_000).live,
      2,
      'upsert replaces, never duplicates',
    );

    // A hard-killed owner never marks its rows; a dead pid makes them lost, not live.
    reg.upsert(row('orphan', 'a', 'a', 'running', {pid: 999_999}));
    assert.equal(
      reg.subtree('a', 100_000, 90_000, pid => pid !== 999_999).live,
      2,
    );
    assert.equal(reg.get('orphan').state, 'lost');
    assert.equal(
      reg.children('a', () => true).find(r => r.id === 'a2').state,
      'asking',
      'live owners untouched',
    );

    reg.event({worker: 'a1', at: 1, kind: 'call', text: 'bash ls'});
    reg.event({worker: 'a1', at: 2, kind: 'said', text: 'x'.repeat(5000)});
    const events = reg.events('a1', 10);
    assert.equal(events.length, 2);
    assert.equal(events[0].kind, 'said', 'most recent first');
    assert.equal(events[0].text.length, 2000, 'event text is bounded');
    for (let at = 0; at < 250; at++)
      reg.event({worker: 'bounded', at, kind: 'note', text: 'event'});
    assert.equal(reg.events('bounded', 1000).length, 200);
    assert.equal(reg.events('bounded', 1000).at(-1).at, 50);
    reg.upsert(row('cycle1', 'cycle2', 'cycle1', 'running'));
    reg.upsert(row('cycle2', 'cycle1', 'cycle1', 'running'));
    assert.ok(
      reg.subtree('cycle1', 100_000, 90_000).live <= 2,
      'corrupt cycles terminate',
    );
    reg.close();
    reg.close();
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
});

test('task observations preserve project scope and multiple contributors across reopen', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'dots-registry-task-'));
  const file = path.join(dir, 'workers.db');
  let reg;
  try {
    reg = openRegistry(file, assert.fail);
    reg.upsert(
      row('writer', null, 'writer', 'completed', {
        task_project: '/project',
        task_id: 't-123abc',
      }),
    );
    reg.upsert(
      row('reviewer', null, 'reviewer', 'paused', {
        task_project: '/project',
        task_id: 't-123abc',
      }),
    );
    reg.upsert(
      row('elsewhere', null, 'elsewhere', 'completed', {
        task_project: '/other',
        task_id: 't-123abc',
      }),
    );
    reg.close();
    reg = openRegistry(file, assert.fail);
    assert.deepEqual(
      new Set(
        readTaskWorkers('/project', ['t-123abc'], file).rows.map(r => r.id),
      ),
      new Set(['writer', 'reviewer']),
    );
    assert.deepEqual(readTaskWorkers('/project', ['t-abcdef'], file), {
      rows: [],
      truncated: false,
    });
    assert.match(
      readTaskWorkers('/project', ['t-123abc'], path.join(dir, 'absent.db'))
        .warning,
      /unavailable/,
    );
    assert.equal(reg.get('writer').task_project, '/project');
    reg.upsert(
      row('stale', null, 'stale', 'running', {
        task_project: '/project',
        task_id: 't-dead',
        pid: 2_000_000_000,
      }),
    );
    assert.equal(
      readTaskWorkers('/project', ['t-dead'], file).rows[0].ownerAvailable,
      false,
    );
    assert.equal(
      reg.get('stale').state,
      'running',
      'read-only observer must not rewrite records',
    );
    for (let i = 0; i < 22; i++)
      reg.upsert(
        row(`many-${i}`, null, 'many', 'completed', {
          task_project: '/project',
          task_id: 't-many',
        }),
      );
    const bounded = readTaskWorkers('/project', ['t-many'], file);
    assert.equal(bounded.rows.length, 20);
    assert.equal(bounded.truncated, true);
    reg.upsert(row('writer', null, 'writer', 'completed'));
    assert.deepEqual(
      readTaskWorkers('/project', ['t-123abc'], file).rows.map(r => r.id),
      ['reviewer'],
    );
  } finally {
    reg?.close();
    rmSync(dir, {recursive: true, force: true});
  }
});

test('task observations decode each row; an unrenderable timestamp is omitted with a warning', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'dots-registry-decode-'));
  const file = path.join(dir, 'workers.db');
  const reg = openRegistry(file, assert.fail);
  try {
    reg.upsert(
      row('good', null, 'good', 'completed', {
        task_project: '/project',
        task_id: 't-abc',
        updated_at: 1_700_000_000_000,
      }),
    );
    // Another runtime's writer is outside our control: a value Date cannot render must not reach goal's renderer.
    reg.upsert(
      row('future', null, 'future', 'completed', {
        task_project: '/project',
        task_id: 't-abc',
        updated_at: 8_640_000_000_000_001,
        started_at: 2,
      }),
    );
    reg.upsert(
      row('fraction', null, 'fraction', 'completed', {
        task_project: '/project',
        task_id: 't-abc',
        updated_at: 1.5,
        started_at: 1,
      }),
    );
    const seen = readTaskWorkers('/project', ['t-abc'], file);
    assert.deepEqual(
      seen.rows.map(r => r.id),
      ['good'],
    );
    assert.equal(
      new Date(seen.rows[0].updated_at).toISOString(),
      '2023-11-14T22:13:20.000Z',
    );
    assert.equal(
      seen.warning,
      '2 worker observations were malformed and omitted',
    );
  } finally {
    reg.close();
    rmSync(dir, {recursive: true, force: true});
  }
});

test('lock contention is transient; other database failures are not', async () => {
  const {isContention} = await import('../extensions/workers/registry.ts');
  const {DatabaseSync} = await import('node:sqlite');
  const dir = mkdtempSync(path.join(tmpdir(), 'dots-registry-busy-'));
  const file = path.join(dir, 'workers.db');
  const holder = new DatabaseSync(file);
  const reg = openRegistry(file, assert.fail);
  const blocked = new DatabaseSync(file);
  try {
    blocked.exec('pragma busy_timeout = 1');
    holder.exec('begin immediate');
    let busy;
    try {
      blocked.exec('begin immediate');
    } catch (error) {
      busy = error;
    }
    assert.ok(busy, 'a second writer is refused while the lock is held');
    assert.equal(isContention(busy), true);
    holder.exec('commit');
    assert.equal(isContention(new Error('disk I/O error')), false);
    assert.equal(
      isContention(Object.assign(new Error('malformed'), {errcode: 11})),
      false,
      'SQLITE_CORRUPT is not contention',
    );
    assert.equal(
      isContention(Object.assign(new Error('snapshot'), {errcode: 517})),
      true,
      'extended SQLITE_BUSY codes are contention',
    );
  } finally {
    blocked.close();
    holder.close();
    reg.close();
    rmSync(dir, {recursive: true, force: true});
  }
});

test('forgetting a pruned worker removes its rows; owner liveness reads its row', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'dots-registry-forget-'));
  const reg = openRegistry(path.join(dir, 'workers.db'), assert.fail);
  try {
    reg.upsert(row('a', null, 'a', 'completed'));
    reg.upsert(row('b', null, 'b', 'completed'));
    reg.event({worker: 'a', at: 1, kind: 'state', text: 'completed'});
    assert.equal(
      reg.ownerAlive('a', () => true),
      true,
    );
    assert.equal(
      reg.ownerAlive('a', () => false),
      false,
    );
    assert.equal(
      reg.ownerAlive('missing', () => true),
      false,
    );
    reg.forget('a');
    assert.equal(reg.get('a'), undefined);
    assert.deepEqual(reg.events('a', 10), []);
    assert.ok(reg.get('b'));
  } finally {
    reg.close();
    rmSync(dir, {recursive: true, force: true});
  }
});
