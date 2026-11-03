import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {PassThrough} from 'node:stream';
import test from 'node:test';
import {Rex, RexError, parseRexEvent, rexBinary} from '../src/host/rex.ts';
import {NdjsonStream} from '../src/host/stream.ts';
import {fakeClock} from './wake-fixture.mjs';

const settle = async () => {
  for (let i = 0; i < 5; i++) await new Promise(r => setImmediate(r));
};

/** An in-process child: the test writes its output and decides when it exits. */
function fakeChild({obeysTerm = true} = {}) {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.exitCode = null;
  child.signalCode = null;
  child.signals = [];
  child.exit = (code, signal = null) => {
    if (child.exitCode !== null || child.signalCode !== null) return;
    child.exitCode = code;
    child.signalCode = signal;
    child.stdout.end();
    child.stderr.end();
    setImmediate(() => child.emit('close', code, signal));
  };
  child.kill = signal => {
    child.signals.push(signal);
    if (signal === 'SIGKILL' || obeysTerm) child.exit(null, signal);
    return true;
  };
  child.spawned = () => child.emit('spawn');
  return child;
}

function launcher(options) {
  const children = [];
  const calls = [];
  const launch = (file, args) => {
    const child = fakeChild(options);
    children.push(child);
    calls.push([file, ...args]);
    return child;
  };
  return {launch, children, calls};
}

test('the stream starts lazily, parses lines, and reports connection state', async () => {
  const clock = fakeClock();
  const {launch, children, calls} = launcher();
  const stream = new NdjsonStream({
    file: 'tool',
    args: ['follow'],
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
  await settle();
  assert.deepEqual(lines, [{a: 1}, {b: 2}, {c: 3}]);
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
  const stream = new NdjsonStream({file: 'tool', args: [], clock, launch});
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
    args: [],
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
    args: [],
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

test('the Rex CLI comes from REX_BIN_DIR, else PATH', () => {
  assert.equal(rexBinary({REX_BIN_DIR: '/opt/rex/bin'}), '/opt/rex/bin/rex');
  assert.equal(rexBinary({}), 'rex');
});

const json = value => ({stdout: JSON.stringify(value), stderr: '', code: 0});
const apiError = (message, code) => ({
  stdout: '',
  stderr: `Error: stream response "control-1" failed: ${message} (status=404 code=${code})\n`,
  code: 3,
});

/** Answers control API calls by method; records every invocation. */
function fakeExec(answer) {
  const calls = [];
  const exec = async (file, args) => {
    const at = args.indexOf('call');
    const method = args[at + 1];
    const params = JSON.parse(args[at + 2]);
    calls.push({file, args, method, params});
    return answer(method, params);
  };
  return {exec, calls};
}

const S = 'session:01a1-0000-work';
const B = 'block:01a1-0000-0000build';
const resolved = (kind, target) => ({
  results: [{query: 'q', status: 'resolved', target: {kind, ...target}}],
});
const blocks = {
  blocks: [
    {block_id: B, label: 'build', creator_name: 'com.superlogical.terminal'},
    {
      block_id: 'block:01a1-0000-0000guide',
      label: 'guide',
      creator_name: 'com.superlogical.onboarding',
    },
  ],
};

test('resolvePane names one terminal block or lists the candidates', async () => {
  let blockAnswer = resolved('block', {
    session_id: S,
    block_id: B,
    label: 'build',
    where: 'window 1',
  });
  const {exec, calls} = fakeExec((method, params) => {
    if (method === 'target.resolve' && params.kinds[0] === 'session')
      return params.queries[0] === 'gone'
        ? json({results: [{query: 'gone', status: 'not_found'}]})
        : json(resolved('session', {session_id: S, label: 'work'}));
    if (method === 'session.list')
      return json({sessions: [{session_id: S, label: 'work'}]});
    if (method === 'target.resolve') return json(blockAnswer);
    if (method === 'session.list_blocks') return json(blocks);
    throw new Error(`unexpected ${method}`);
  });
  const rex = new Rex({env: {REX_BIN_DIR: '/opt/rex'}, exec});
  assert.deepEqual(
    await rex.resolvePane({target: 'build', session: 'work', host: 'studio'}),
    {
      host: 'studio',
      session: S,
      sessionLabel: 'work',
      block: B,
      label: 'build',
      where: 'window 1',
    },
  );
  assert.deepEqual(calls[0].args.slice(0, 5), [
    '--autostart=false',
    '--server',
    'studio',
    'api',
    'call',
  ]);
  assert.equal(calls[0].file, '/opt/rex/rex');
  assert.deepEqual(calls[1].params, {
    kinds: ['block'],
    queries: ['build'],
    scope: {session: S},
  });
  await assert.rejects(
    rex.resolvePane({target: 'build', session: 'gone'}),
    /no Rex session "gone"; sessions: work \(000-work\)/,
  );
  blockAnswer = {
    results: [
      {
        query: '0000',
        status: 'ambiguous',
        candidates: [
          {kind: 'block', label: 'build', block_id: B, where: 'window 1'},
          {
            kind: 'block',
            label: '',
            block_id: 'block:01a1-0000-0000other',
            where: 'window 2',
          },
        ],
      },
    ],
  };
  await assert.rejects(
    rex.resolvePane({target: '0000', session: 'work'}),
    /matches several blocks in work: build \(000build\) in window 1, \(no label\) \(000other\) in window 2/,
  );
  blockAnswer = {results: [{query: 'nope', status: 'not_found'}]};
  await assert.rejects(
    rex.resolvePane({target: 'nope', session: 'work'}),
    /no block "nope" in session work; blocks: build \(000build\), guide \(000guide\)/,
  );
  blockAnswer = resolved('block', {
    session_id: S,
    block_id: 'block:01a1-0000-0000guide',
    label: 'guide',
  });
  await assert.rejects(
    rex.resolvePane({target: 'guide', session: 'work'}),
    /is a com.superlogical.onboarding block, not a terminal/,
  );
});

test('snapshotPane reads the root record, exit and server run, and maps vanished panes', async () => {
  let status = json({
    records: [
      {id: 'worker/w-1', state: 'working', updated_at: 'u0'},
      {
        state: 'blocked',
        kind: 'question',
        effective_app: 'agent',
        msg: 'Which?',
        updated_at: 'u1',
        owner: {pid: 9, name: 'agent'},
      },
    ],
  });
  let server = json({instance_id: 'run-1', pid: 1});
  const proc = json({
    last_exit: {exit_code: 2, error: 'exit status 2'},
    child: {pid: 41, name: 'make', user_id: 501},
    foreground: null,
  });
  const {exec, calls} = fakeExec((method, params) => {
    if (method === 'server.status') return server;
    if (method === 'block.method')
      return params.method === 'process' ? proc : status;
    throw new Error(method);
  });
  const rex = new Rex({env: {}, exec});
  const snapshot = await rex.snapshotPane({session: S, block: B});
  assert.deepEqual(
    calls.map(call => call.params.method ?? call.method).sort(),
    ['process', 'program_status', 'server.status'],
  );
  assert.deepEqual(snapshot, {
    present: true,
    instance: 'run-1',
    record: {
      state: 'blocked',
      kind: 'question',
      app: 'agent',
      msg: 'Which?',
      updatedAt: 'u1',
      owner: {pid: 9, name: 'agent'},
    },
    exit: {code: 2, error: 'exit status 2'},
    child: {pid: 41, name: 'make'},
  });
  status = apiError(`block not found: ${B}`, 'not_found');
  assert.deepEqual(await rex.snapshotPane({session: S, block: B}), {
    present: false,
    instance: 'run-1',
    reason: 'block closed',
  });
  status = apiError(`session ${S} not found`, 'session_not_found');
  assert.equal(
    (await rex.snapshotPane({session: S, block: B})).reason,
    'session destroyed',
  );
  server = {
    stdout: '',
    stderr:
      'Error: no Rex server is running at /tmp/x.sock, and autostart is disabled\n',
    code: 4,
  };
  const error = await rex.snapshotPane({session: S, block: B}).catch(e => e);
  assert.ok(error instanceof RexError);
  assert.equal(error.code, undefined, 'unreachable is not an API answer');
  assert.match(error.message, /autostart is disabled/);
});

test('Rex events are parsed to the shapes observers use; others are ignored', () => {
  const block = (name, payload) => ({
    type: 'session_event',
    payload: {
      session_id: S,
      event: {
        type: 'block_event',
        payload: {
          block_id: B,
          creator_name: 'com.superlogical.terminal',
          name,
          payload,
        },
      },
    },
  });
  assert.deepEqual(
    parseRexEvent(
      block('program_status_changed', {
        id: '',
        record: {
          state: 'done',
          app: 'agent',
          updated_at: 'u2',
          owner: {pid: 9, name: 'agent'},
          extra: true,
        },
        reason: 'report',
      }),
    ),
    {
      type: 'program_status_changed',
      session: S,
      block: B,
      id: '',
      record: {
        state: 'done',
        app: 'agent',
        updatedAt: 'u2',
        owner: {pid: 9, name: 'agent'},
      },
    },
  );
  assert.deepEqual(
    parseRexEvent(
      block('program_status_removed', {ids: [''], reason: 'prompt'}),
    ),
    {
      type: 'program_status_removed',
      session: S,
      block: B,
      ids: [''],
      reason: 'prompt',
    },
  );
  assert.deepEqual(
    parseRexEvent(
      block('child_exited', {exit_code: 1, error: 'exit status 1'}),
    ),
    {
      type: 'child_exited',
      session: S,
      block: B,
      exit: {code: 1, error: 'exit status 1'},
    },
  );
  assert.deepEqual(
    parseRexEvent({
      type: 'session_event',
      payload: {
        session_id: S,
        event: {type: 'block_closed', payload: {block_id: B}},
      },
    }),
    {type: 'block_closed', session: S, block: B},
  );
  assert.deepEqual(
    parseRexEvent({type: 'session_destroyed', payload: {session_id: S}}),
    {
      type: 'session_destroyed',
      session: S,
    },
  );
  for (const ignored of [
    block('bell', {}),
    block('program_status_changed', {id: '', record: {state: 'done'}}),
    {type: 'client_joined', payload: {}},
    'text',
    null,
  ])
    assert.equal(parseRexEvent(ignored), undefined);
});

test('one --all stream per host, shared by listeners, syncing on its first line or after settling', async () => {
  const clock = fakeClock();
  const {launch, children, calls} = launcher();
  const rex = new Rex({
    env: {},
    exec: async () => json({}),
    launch,
    clock,
    settleMs: 2_000,
  });
  const log = [];
  const listener = name => ({
    open: () => log.push(`${name} open`),
    sync: () => log.push(`${name} sync`),
    event: event => log.push(`${name} ${event.type}`),
    state: state => log.push(`${name} ${state.status}`),
  });
  const stopA = rex.watch(undefined, listener('a'));
  const stopB = rex.watch(undefined, listener('b'));
  const stopRemote = rex.watch('studio', listener('r'));
  assert.deepEqual(calls, [
    ['rex', '--autostart=false', 'events', '--all', '--json'],
    [
      'rex',
      '--autostart=false',
      '--server',
      'studio',
      'events',
      '--all',
      '--json',
    ],
  ]);
  children[0].spawned();
  children[0].stdout.write(
    `${JSON.stringify({type: 'session_destroyed', payload: {session_id: S}})}\n`,
  );
  children[0].stdout.write(
    `${JSON.stringify({type: 'client_left', payload: {}})}\n`,
  );
  await settle();
  assert.deepEqual(log, [
    'a connected',
    'b connected',
    'a open',
    'b open',
    'a session_destroyed',
    'b session_destroyed',
    'a sync',
    'b sync',
  ]);
  log.length = 0;
  children[1].spawned();
  await clock.advance(2_000);
  assert.deepEqual(
    log,
    ['r connected', 'r open', 'r sync'],
    'a quiet stream syncs after settling',
  );
  stopA();
  assert.deepEqual(children[0].signals, [], 'still watched by b');
  stopB();
  await settle();
  assert.deepEqual(children[0].signals, ['SIGTERM']);
  stopRemote();
  await rex.close();
  assert.equal(clock.pending(), 0);
});
