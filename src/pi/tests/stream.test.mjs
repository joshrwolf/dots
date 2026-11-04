import assert from 'node:assert/strict';
import test from 'node:test';
import {NdjsonStream} from '../src/host/stream.ts';
import {fakeChild, launcher} from './child-fixture.mjs';
import {fakeClock} from './wake-fixture.mjs';

const settle = async () => {
  for (let i = 0; i < 5; i++) await new Promise(r => setImmediate(r));
};

test('the stream starts lazily, parses lines, and reports connection state', async () => {
  const clock = fakeClock();
  const {launch, children, calls} = launcher();
  const stream = new NdjsonStream({
    file: 'tool',
    args: () => ['follow'],
    clock,
    launch,
  });
  assert.equal(children.length, 0, 'nothing runs before a subscriber');
  const lines = [];
  const states = [];
  let opened = 0;
  const stop = stream.subscribe({
    open: () => opened++,
    line: value => lines.push(value),
    state: state => states.push(state.status),
  });
  assert.deepEqual(calls, [['tool', 'follow']]);
  children[0].spawned();
  assert.equal(opened, 1);
  assert.equal(stream.state.status, 'connected');
  children[0].stdout.write('{"a":1}\n{"b"');
  children[0].stdout.write(':2}\nnot json\n\n');
  children[0].stdout.write(`${'x'.repeat(2 * 1024 * 1024)}`);
  children[0].stdout.write('still the long line\n{"c":3}\n');
  // A complete oversized line in one chunk is dropped too.
  children[0].stdout.write(
    `{"big":"${'y'.repeat(2 * 1024 * 1024)}"}\n{"d":4}\n`,
  );
  await settle();
  assert.deepEqual(lines, [{a: 1}, {b: 2}, {c: 3}, {d: 4}]);
  stop();
  await settle();
  assert.deepEqual(children[0].signals, ['SIGTERM']);
  assert.deepEqual(
    states,
    ['connected'],
    'a departed subscriber hears nothing more',
  );
  assert.equal(clock.pending(), 0);
});

test('a failed connection retries with doubling backoff that a healthy one resets', async () => {
  const clock = fakeClock();
  const {launch, children} = launcher();
  const stream = new NdjsonStream({
    file: 'tool',
    args: () => [],
    clock,
    launch,
  });
  const states = [];
  stream.subscribe({line: () => {}, state: state => states.push(state)});
  const fail = async (index, stderr) => {
    children[index].stderr.write(stderr);
    await settle();
    children[index].exit(4);
    await settle();
  };
  await fail(0, 'Error: no server is running\n');
  assert.deepEqual(states.at(-1), {
    status: 'disconnected',
    error: 'Error: no server is running',
    retryAt: clock.now() + 1_000,
  });
  await clock.advance(1_000);
  assert.equal(children.length, 2);
  await fail(1, '');
  assert.match(states.at(-1).error, /exited with code 4/);
  assert.equal(states.at(-1).retryAt, clock.now() + 2_000);
  for (const delay of [2_000, 4_000, 8_000, 16_000, 32_000, 60_000]) {
    await clock.advance(delay);
    await fail(children.length - 1, 'down');
  }
  assert.equal(states.at(-1).retryAt, clock.now() + 60_000, 'capped');
  await clock.advance(60_000);
  children.at(-1).spawned();
  await clock.advance(30_000);
  await fail(children.length - 1, 'dropped');
  assert.equal(
    states.at(-1).retryAt,
    clock.now() + 1_000,
    'reset after a healthy connection',
  );
  await stream.close();
  assert.equal(clock.pending(), 0);
});

test('close stops retries, escalates to SIGKILL, and waits for every exit', async () => {
  const clock = fakeClock();
  const {launch, children} = launcher({obeysTerm: false});
  const stream = new NdjsonStream({
    file: 'tool',
    args: () => [],
    clock,
    launch,
    stopGraceMs: 500,
  });
  stream.subscribe({line: () => {}});
  children[0].spawned();
  let closed = false;
  const closing = stream.close().then(() => {
    closed = true;
  });
  await settle();
  assert.deepEqual(children[0].signals, ['SIGTERM']);
  assert.equal(closed, false);
  await clock.advance(500);
  await closing;
  assert.deepEqual(children[0].signals, ['SIGTERM', 'SIGKILL']);
  assert.equal(stream.active, false);
  assert.equal(clock.pending(), 0);
  // A spawn failure is a disconnection, not a crash.
  const broken = new NdjsonStream({
    file: 'missing',
    args: () => [],
    clock,
    launch: () => {
      const child = fakeChild();
      setImmediate(() =>
        child.emit('error', new Error('spawn missing ENOENT')),
      );
      return child;
    },
  });
  const states = [];
  broken.subscribe({line: () => {}, state: s => states.push(s)});
  await settle();
  assert.equal(states.at(-1).error, 'spawn missing ENOENT');
  await broken.close();
  assert.equal(clock.pending(), 0);
});
