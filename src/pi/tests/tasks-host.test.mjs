import assert from 'node:assert/strict';
import test from 'node:test';
import {DotsError} from '@joshrwolf/pi-kit/dots';
import {Tasks, parseTaskLine} from '../src/host/tasks.ts';
import {launcher} from './child-fixture.mjs';
import {fakeClock} from './wake-fixture.mjs';

const settle = async () => {
  for (let i = 0; i < 5; i++) await new Promise(r => setImmediate(r));
};

const PROJECT = 'p-0000000000000000000000000000beef';
const event = (seq, extra = {}) => ({
  seq,
  task: 't-a0',
  at: 1_700_000_000,
  actor: 'pi-other',
  kind: 'review',
  detail: null,
  result: 'Ready',
  predecessor: null,
  operation: 'review',
  ...extra,
});

test('stream lines are events, a storage failure, or ignored', () => {
  assert.deepEqual(parseTaskLine(event(3)), {event: event(3)});
  assert.deepEqual(parseTaskLine({error: 'database is locked'}), {
    error: 'database is locked',
  });
  assert.equal(parseTaskLine({...event(3), task: 'not-a-task'}), undefined);
  assert.equal(parseTaskLine({...event(3), seq: 'x'}), undefined);
  assert.equal(parseTaskLine([1]), undefined);
});

test('one-shot calls go through the dots client and are validated', async () => {
  const calls = [];
  let kindsFail = true;
  const run = async (args, options) => {
    calls.push({args, directory: options.directory, env: options.env});
    if (args[1] === 'event-kinds') {
      if (kindsFail) throw new Error('dots: unrecognized subcommand');
      return {kinds: ['created', 'review', 'done']};
    }
    if (args[1] === 'show')
      return {
        project: PROJECT,
        task: {id: args[2], parent: null, title: 'Goal', status: 'open'},
        now: 1,
      };
    if (args[1] === 'events')
      return {events: [], head: 1002, project: PROJECT, now: 1};
    throw new Error(`unexpected ${args.join(' ')}`);
  };
  const env = {XDG_DATA_HOME: '/tmp/fixture-data'};
  const tasks = new Tasks({run, env});
  await assert.rejects(tasks.kinds(), /unrecognized subcommand/);
  kindsFail = false;
  assert.deepEqual(await tasks.kinds(), ['created', 'review', 'done']);
  await tasks.kinds();
  assert.equal(
    calls.filter(c => c.args[1] === 'event-kinds').length,
    2,
    'a failure is not cached; a success is',
  );
  assert.equal(calls[0].env, env);

  assert.deepEqual(await tasks.task('t-a0', {directory: '/checkout'}), {
    project: PROJECT,
    id: 't-a0',
    parent: null,
    title: 'Goal',
    status: 'open',
  });
  assert.deepEqual(calls.at(-1).args, ['task', 'show', 't-a0']);
  assert.equal(calls.at(-1).directory, '/checkout');
  await tasks.task('t-a0', {project: PROJECT});
  assert.deepEqual(calls.at(-1).args, [
    'task',
    'show',
    't-a0',
    '--project',
    PROJECT,
  ]);

  assert.equal(await tasks.head(PROJECT), 1002);
  assert.deepEqual(
    calls.at(-1).args,
    ['task', 'events', '--project', PROJECT, '--limit', '0'],
    'the head is one small call',
  );
  assert.equal(calls.at(-1).env, env);

  const bad = new Tasks({run: async () => ({project: PROJECT, task: {}})});
  await assert.rejects(
    bad.task('t-a0', {project: PROJECT}),
    /dots task show returned an unexpected shape/,
  );
});

test('history pages read one subtree and halve when a page outgrows the output cap', async () => {
  const calls = [];
  const run = async args => {
    const limit = Number(args[args.indexOf('--limit') + 1]);
    calls.push({since: args[args.indexOf('--since') + 1], limit});
    if (limit > 25) {
      const cause = Object.assign(
        new RangeError('stdout maxBuffer length exceeded'),
        {code: 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER'},
      );
      throw new DotsError(args, 'failed', '', cause.message, {cause});
    }
    return {
      events: Array.from({length: limit}, (_, i) => event(i + 1)),
      head: 99,
    };
  };
  const tasks = new Tasks({run});
  const page = await tasks.events(PROJECT, 't-a0', 4);
  assert.equal(page.events.length, 25);
  assert.equal(page.more, true);
  assert.deepEqual(
    calls.map(c => c.limit),
    [100, 50, 25],
  );
  await tasks.events(PROJECT, 't-a0', 29);
  assert.deepEqual(
    calls.at(-1),
    {since: '29', limit: 25},
    'the smaller page sticks',
  );

  const failing = new Tasks({
    run: async args => {
      throw new DotsError(
        args,
        'failed',
        'dots: task t-a0 not found',
        'dots: task t-a0 not found',
      );
    },
  });
  await assert.rejects(failing.events(PROJECT, 't-a0', 0), /not found/);
  const args = [];
  await new Tasks({
    run: async a => {
      args.push(a);
      return {events: [], head: 0};
    },
  }).events(PROJECT, 't-a0', 4);
  assert.deepEqual(args[0], [
    'task',
    'events',
    '--project',
    PROJECT,
    '--under',
    't-a0',
    '--since',
    '4',
    '--limit',
    '100',
  ]);
});

test('a follower reads one subtree in the configured environment, resumes from the cursor at each spawn and reports storage failures', async () => {
  const clock = fakeClock();
  const {launch, children, calls, spawns} = launcher();
  const env = {XDG_DATA_HOME: '/tmp/fixture-data'};
  const tasks = new Tasks({launch, clock, env});
  let cursor = 7;
  const events = [];
  const states = [];
  const stop = tasks.follow(PROJECT, 't-a0', () => cursor, {
    event: e => events.push(e.seq),
    state: s => states.push(s),
  });
  assert.deepEqual(calls, [
    [
      'dots',
      '--json',
      'task',
      'events',
      '--follow',
      '--project',
      PROJECT,
      '--under',
      't-a0',
      '--since',
      '7',
    ],
  ]);
  assert.equal(
    spawns[0].env,
    env,
    'the follower reads the same catalog as calls',
  );
  children[0].spawned();
  children[0].stdout.write(
    `${JSON.stringify(event(8))}\n{"other":1}\n${JSON.stringify(event(9))}\n`,
  );
  await settle();
  assert.deepEqual(events, [8, 9]);
  cursor = 9;
  children[0].stdout.write('{"error":"database disk image is malformed"}\n');
  children[0].stderr.write('dots: database disk image is malformed\n');
  await settle();
  children[0].exit(1);
  await settle();
  assert.equal(states.at(-1).status, 'disconnected');
  assert.equal(states.at(-1).error, 'database disk image is malformed');
  await clock.advance(1_000);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].at(-1), '9', 'the restart resumes from the cursor');
  children[1].spawned();
  children[1].exit(1);
  await settle();
  assert.match(
    states.at(-1).error,
    /exited with code 1/,
    'a failure line belongs to its own connection',
  );
  await clock.advance(2_000);
  children[2].spawned();

  stop();
  assert.deepEqual(children[2].signals, ['SIGTERM']);
  await tasks.close();
  assert.ok(
    children.every(
      child => child.exitCode !== null || child.signalCode !== null,
    ),
  );
});

test('close aborts calls in flight and waits for followers to exit', async () => {
  const {launch, children} = launcher({obeysTerm: false});
  const clock = fakeClock();
  let aborted = false;
  const run = (_args, options) =>
    new Promise((_, reject) => {
      options.signal.addEventListener('abort', () => {
        aborted = true;
        reject(new Error('aborted'));
      });
    });
  const tasks = new Tasks({run, launch, clock});
  const lookup = tasks.task('t-a0', {project: PROJECT}).catch(() => 'rejected');
  tasks.follow(PROJECT, 't-a0', () => 0, {event() {}, state() {}});
  children[0].spawned();
  let closed = false;
  const closing = tasks.close().then(() => {
    closed = true;
  });
  await settle();
  assert.equal(aborted, true);
  assert.equal(await lookup, 'rejected');
  assert.equal(closed, false, 'the follower ignored SIGTERM');
  await clock.advance(2_000);
  await closing;
  assert.deepEqual(children[0].signals, ['SIGTERM', 'SIGKILL']);
});
