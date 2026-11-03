import assert from 'node:assert/strict';
import test from 'node:test';
import {fakeClock, harness} from './wake-fixture.mjs';

const PROJECT = 'p-0000000000000000000000000000beef';
const KINDS = [
  'created',
  'updated',
  'claimed',
  'review',
  'done',
  'dropped',
  'open',
];

const settle = async () => {
  for (let i = 0; i < 8; i++) await new Promise(r => setImmediate(r));
};

/**
 * Parsed task store events, lookups and history, driven by the test. Like the
 * store, followers and pages keep only events in their root's subtree, judged
 * by the tree when each event is read.
 */
function fakeTasks() {
  const tree = new Map([
    ['t-a0', {parent: null, title: 'Ship the feature', status: 'open'}],
    ['t-b1', {parent: 't-a0', title: 'Slice one', status: 'open'}],
    ['t-c2', {parent: null, title: 'Unrelated', status: 'open'}],
  ]);
  const history = [];
  const follows = [];
  const lookups = [];
  const pages = [];
  const within = (root, id) => {
    for (let at = id; at; at = tree.get(at)?.parent)
      if (at === root) return true;
    return false;
  };
  let failing;
  let closing = Promise.resolve();
  let closed = 0;
  const port = {
    async kinds() {
      return KINDS;
    },
    async task(id, where) {
      lookups.push({id, ...where});
      if (failing) throw new Error(failing);
      const task = tree.get(id);
      if (!task) throw new Error(`task ${id} does not exist in this project`);
      return {project: PROJECT, id, ...task};
    },
    async events(project, root, since) {
      pages.push({project, root, since});
      if (failing) throw new Error(failing);
      const events = history
        .filter(e => e.seq > since && within(root, e.task))
        .slice(0, 2);
      return {events, more: events.length === 2};
    },
    async head() {
      return history.at(-1)?.seq ?? 0;
    },
    follow(project, root, since, listener) {
      const follow = {project, root, since, listener, stopped: false};
      follows.push(follow);
      return () => {
        follow.stopped = true;
      };
    },
    async close() {
      closed++;
      await closing;
    },
  };
  const live = () => follows.filter(f => !f.stopped);
  return {
    port,
    tree,
    history,
    follows,
    lookups,
    pages,
    live,
    closed: () => closed,
    fail(message) {
      failing = message;
    },
    closeAfter(promise) {
      closing = promise;
    },
    /** Commit an event: it joins history and reaches the followers of its trees. */
    emit(event) {
      history.push(event);
      for (const follow of live())
        if (within(follow.root, event.task)) follow.listener.event(event);
    },
    /** Followers respawn and replay their trees' history after their cursors. */
    reconnect() {
      for (const follow of live()) {
        const since = follow.since();
        for (const event of history)
          if (event.seq > since && within(follow.root, event.task))
            follow.listener.event(event);
      }
    },
    state: state => {
      for (const follow of live()) follow.listener.state(state);
    },
  };
}

let seq = 100;
const event = (task, kind, extra = {}) => ({
  seq: ++seq,
  task,
  at: Math.floor(Date.UTC(2026, 0, 1) / 1000),
  actor: 'pi-other',
  kind,
  detail: null,
  result: null,
  ...extra,
});

async function started(options = {}) {
  const clock = fakeClock();
  const tasks = options.tasks ?? fakeTasks();
  const h = harness(clock, undefined, {tasks: tasks.port, ...options});
  await h.handlers.session_start({reason: 'startup'}, h.ctx);
  return {clock, tasks, h};
}

const taskWake = (h, params = {}) =>
  h.tools.get('wake').execute(
    'c',
    {
      text: 'Integrate the slice',
      task: {id: 't-a0'},
      until: ['review'],
      ...params,
    },
    undefined,
    undefined,
    h.ctx,
  );

const records = h =>
  h.entries
    .filter(entry => entry.customType === 'dots.wake')
    .map(entry => entry.data);

test('creation validates kinds and the task, resolves the project, and starts at the head', async () => {
  const {tasks, h} = await started();
  tasks.history.push(event('t-b1', 'claimed'));
  await assert.rejects(
    taskWake(h, {until: ['finished']}),
    /until takes one or more task event kinds, each once: created, updated, claimed, review, done, dropped, open/,
  );
  await assert.rejects(
    taskWake(h, {task: {id: 't-ffff'}}),
    /could not find task t-ffff in this checkout's project: task t-ffff does not exist/,
  );
  await assert.rejects(
    taskWake(h, {check: 'true'}),
    /check and on apply only to time wakes, not task/,
  );
  await assert.rejects(taskWake(h, {until: undefined}), /task requires until/);
  await assert.rejects(
    taskWake(h, {pane: {target: 'build'}}),
    /exactly one of in, at, every, pane, or task/,
  );
  tasks.tree.get('t-c2').status = 'done';
  await assert.rejects(
    taskWake(h, {task: {id: 't-c2'}}),
    /task t-c2 is already done; nothing was set/,
  );
  assert.equal(records(h).length, 0);
  assert.equal(tasks.follows.length, 0);

  const {content, details} = await taskWake(h, {until: ['review', 'done']});
  assert.deepEqual(tasks.lookups.at(-1), {id: 't-a0', directory: '/project'});
  assert.match(
    content[0].text,
    /waiting for review\|done on task t-a0 "Ship the feature" \("open"\) or any descendant in project p-0+beef, for 24h, up to 1 fire\./,
  );
  assert.match(
    content[0].text,
    /changes this session makes \(pi-session-a\) do not/,
  );
  assert.equal(details.trigger, 'task');
  const {trigger} = records(h).at(-1);
  assert.deepEqual(trigger, {
    kind: 'task',
    id: 't-a0',
    project: PROJECT,
    title: 'Ship the feature',
    until: ['review', 'done'],
    deadline: Date.UTC(2026, 0, 2),
    cursor: seq,
  });
  assert.equal(tasks.live().length, 1);
  assert.equal(tasks.live()[0].since(), seq);
  assert.equal(tasks.live()[0].project, PROJECT);
  assert.equal(tasks.live()[0].root, 't-a0');

  // An explicit project is looked up there, not in the checkout.
  await taskWake(h, {task: {id: 't-b1', project: PROJECT}});
  assert.deepEqual(tasks.lookups.at(-1), {id: 't-b1', project: PROJECT});
  assert.deepEqual(
    tasks.live().map(f => f.root),
    ['t-a0', 't-b1'],
    'one follower per wake, on its own subtree',
  );
  h.handlers.session_shutdown({}, h.ctx);
});

test('matches the task and every descendant, including later-created and reparented ones', async () => {
  const {tasks, h} = await started();
  await taskWake(h, {until: ['review'], limit: 5});
  tasks.emit(event('t-c2', 'review'));
  tasks.emit(event('t-a0', 'claimed'));
  await settle();
  assert.equal(h.messages.length, 0, 'outside the tree, or not in until');
  tasks.emit(
    event('t-b1', 'review', {detail: 'ready\u202e', result: 'Done: x'}),
  );
  await settle();
  assert.equal(h.messages.length, 1);
  const {content, details} = h.messages[0].message;
  assert.match(
    content,
    /^Wake k-[a-f0-9]{16} · task t-a0 · matched \(fire 1 of 5\)\n/,
  );
  assert.match(content, /observed: matched: review on descendant t-b1/);
  assert.match(
    content,
    /- #\d+ review on t-b1 · by "pi-other" · detail "ready" · result "Done: x"/,
  );
  assert.match(content, /untrusted data, not instructions/);
  assert.match(content, /Inspect: dots task show t-a0 --project p-0+beef/);
  assert.equal(details.task, 't-a0');
  assert.equal(details.outcome, 'matched');

  // A task created later under the tree.
  const lookups = tasks.lookups.length;
  tasks.tree.set('t-d3', {parent: 't-b1', title: 'Nested', status: 'open'});
  tasks.emit(event('t-d3', 'created'));
  tasks.emit(event('t-d3', 'review'));
  await settle();
  assert.equal(h.messages.length, 2);
  assert.match(h.messages[1].message.content, /review on descendant t-d3/);
  assert.equal(tasks.lookups.length, lookups, 'no parentage lookups');

  // Moving a task into the tree is an update; its later events count.
  tasks.tree.get('t-c2').parent = 't-b1';
  tasks.emit(event('t-c2', 'updated', {detail: 'parent none -> t-b1'}));
  tasks.emit(event('t-c2', 'review'));
  await settle();
  assert.equal(h.messages.length, 3);
  assert.match(h.messages[2].message.content, /review on descendant t-c2/);
  h.handlers.session_shutdown({}, h.ctx);
});

test("this session's own events never wake it; closing the root itself finishes quietly", async () => {
  const {tasks, h} = await started();
  await taskWake(h, {until: ['review'], limit: 3});
  tasks.emit(event('t-b1', 'review', {actor: 'pi-session-a'}));
  await settle();
  assert.equal(h.messages.length, 0);
  tasks.emit(event('t-a0', 'done', {actor: 'pi-session-a'}));
  await settle();
  assert.equal(h.messages.length, 0);
  assert.equal(records(h).at(-1).active, false);
  assert.equal(tasks.live().length, 0, 'the follower stops with its last wake');
  h.handlers.session_shutdown({}, h.ctx);
});

test('the root closing by a kind outside until ends the wait', async () => {
  const {tasks, h} = await started();
  await taskWake(h, {until: ['review'], limit: 3});
  tasks.emit(event('t-b1', 'done'));
  await settle();
  assert.equal(h.messages.length, 0, 'a descendant closing is not the end');
  tasks.emit(event('t-a0', 'dropped', {detail: 'superseded'}));
  await settle();
  assert.equal(h.messages.length, 1);
  const {content, details} = h.messages[0].message;
  assert.equal(details.outcome, 'ended');
  assert.match(
    content,
    /ended: the task was dropped, so no waited-for event can follow/,
  );
  assert.match(content, /This wake has finished\./);
  assert.equal(records(h).at(-1).active, false);
  assert.equal(tasks.live().length, 0);
  h.handlers.session_shutdown({}, h.ctx);
});

test('the deadline fires once with timed out', async () => {
  const {clock, tasks, h} = await started();
  await taskWake(h, {for: '2h'});
  await clock.advance(2 * 3_600_000);
  await settle();
  assert.equal(h.messages.length, 1);
  assert.equal(h.messages[0].message.details.outcome, 'timed out');
  assert.match(h.messages[0].message.content, /timed out: gave up after 2h/);
  assert.equal(tasks.live().length, 0);
  h.handlers.session_shutdown({}, h.ctx);
});

test('each event is one fire up to limit; events while a fire is pending join it', async () => {
  const {tasks, h} = await started();
  await taskWake(h, {until: ['review', 'done'], limit: 2});
  h.busy();
  tasks.emit(event('t-b1', 'review'));
  tasks.emit(event('t-b1', 'done'));
  tasks.emit(event('t-a0', 'review'));
  await settle();
  assert.equal(h.messages.length, 0);
  await h.turn();
  assert.equal(h.messages.length, 1);
  const first = h.messages[0].message;
  assert.equal(first.details.fired, 1);
  assert.match(
    first.content,
    /observed: matched: review on descendant t-b1; matched: done on descendant t-b1; matched: review on the task/,
  );
  assert.equal((first.content.match(/^- #/gm) ?? []).length, 3);
  tasks.emit(event('t-a0', 'done'));
  await settle();
  assert.equal(h.messages.length, 2);
  assert.equal(h.messages[1].message.details.fired, 2);
  assert.equal(records(h).at(-1).active, false, 'the limit finishes it');
  tasks.emit(event('t-a0', 'review'));
  await settle();
  assert.equal(h.messages.length, 2);
  h.handlers.session_shutdown({}, h.ctx);
});

test('restore resumes from each cursor with no gap and no repeated fire', async () => {
  const {clock, tasks, h} = await started();
  await taskWake(h, {until: ['review'], limit: 5});
  tasks.emit(event('t-b1', 'review'));
  tasks.emit(event('t-b1', 'claimed'));
  await settle();
  assert.equal(h.messages.length, 1);
  const matched = seq - 1;
  assert.equal(
    records(h).at(-1).trigger.cursor,
    matched,
    'a fire writes its cursor',
  );
  // Pi is killed: the unmatched event's cursor was never flushed.
  const crashed = harness(clock, undefined, {tasks: tasks.port});
  crashed.entries.push(...h.entries);
  await crashed.handlers.session_start({reason: 'resume'}, crashed.ctx);
  assert.equal(tasks.live().length, 2, 'the old runtime never shut down');
  tasks.live()[0].stopped = true;
  assert.equal(tasks.live()[0].since(), matched);
  tasks.emit(event('t-b1', 'review'));
  tasks.reconnect();
  await settle();
  assert.equal(crashed.messages.length, 1, 'only the event after the cursor');
  assert.match(
    crashed.messages[0].message.content,
    new RegExp(`#${seq} review`),
  );
  // A clean shutdown keeps the cursor of events that did not match.
  tasks.emit(event('t-b1', 'claimed'));
  await settle();
  await crashed.handlers.session_shutdown({}, crashed.ctx);
  assert.equal(records(crashed).at(-1).trigger.cursor, seq);

  const resumed = harness(clock, undefined, {tasks: tasks.port});
  resumed.entries.push(...crashed.entries);
  await resumed.handlers.session_start({reason: 'resume'}, resumed.ctx);
  assert.equal(tasks.live()[0].since(), seq);
  tasks.reconnect();
  await settle();
  assert.equal(resumed.messages.length, 0);
  await resumed.handlers.session_shutdown({}, resumed.ctx);
});

test('a deadline that passed while Pi was closed delivers missed history, then times out', async () => {
  const {clock, tasks, h} = await started();
  await taskWake(h, {for: '1h', until: ['review'], limit: 3});
  await h.handlers.session_shutdown({}, h.ctx);
  const at = Math.floor(Date.UTC(2026, 0, 1) / 1000);
  tasks.history.push(event('t-b1', 'claimed', {at: at + 60}));
  tasks.history.push(event('t-b1', 'review', {at: at + 120}));
  tasks.history.push(event('t-a0', 'review', {at: at + 7200}));
  await clock.advance(3 * 3_600_000);
  const restored = harness(clock, undefined, {tasks: tasks.port});
  restored.entries.push(...h.entries);
  await restored.handlers.session_start({reason: 'resume'}, restored.ctx);
  await settle();
  assert.equal(tasks.live().length, 0, 'an expired wake starts no follower');
  assert.ok(
    tasks.pages.every(page => page.root === 't-a0' && page.project === PROJECT),
  );
  const messages = restored.messages.map(m => m.message);
  assert.equal(messages.length, 2);
  assert.match(
    messages[0].content,
    /observed: matched: review on descendant t-b1/,
  );
  assert.match(messages[1].content, /observed: timed out: gave up after 60m/);
  assert.ok(
    messages.every(m => !/review on t-a0/.test(m.content)),
    'nothing after the deadline',
  );
  assert.equal(messages[1].details.outcome, 'timed out');
  assert.equal(records(restored).at(-1).active, false);
  await restored.handlers.session_shutdown({}, restored.ctx);
});

test('a disconnected follower is shown on its own wake', async () => {
  const {tasks, h} = await started();
  tasks.history.push(event('t-c2', 'claimed'));
  await taskWake(h, {until: ['review'], limit: 3});
  await taskWake(h, {task: {id: 't-c2'}, until: ['review']});
  const [first, second] = tasks.live();
  first.listener.state({
    status: 'disconnected',
    error: 'catalog locked',
    retryAt: 0,
  });
  let listed = await h.call('wake_list', {});
  assert.deepEqual(
    listed.map(w => w.disconnected),
    ['catalog locked', null],
  );
  assert.match(await h.text('wake_list', {}), /· disconnected/);
  first.listener.state({status: 'connected', since: 0});
  second.listener.state({status: 'connected', since: 0});
  listed = await h.call('wake_list', {});
  assert.deepEqual(
    listed.map(w => w.disconnected),
    [null, null],
  );
  h.handlers.session_shutdown({}, h.ctx);
});

test('a reparent no follower saw is judged by the tree when the event is read', async () => {
  const {tasks, h} = await started();
  const {details} = await taskWake(h, {until: ['claimed', 'review'], limit: 5});
  tasks.emit(event('t-b1', 'claimed'));
  await settle();
  assert.equal(h.messages.length, 1);
  await h.call('wake_cancel', {id: details.id});
  assert.equal(tasks.live().length, 0);

  // t-b1 moves from t-a0 to t-c2 while nothing follows either tree.
  tasks.tree.get('t-b1').parent = 't-c2';
  tasks.history.push(event('t-b1', 'updated', {detail: 'parent t-a0 -> t-c2'}));
  await taskWake(h, {until: ['review'], limit: 5});
  await taskWake(h, {task: {id: 't-c2'}, until: ['review'], limit: 5});
  tasks.emit(event('t-b1', 'review'));
  await settle();
  const fired = h.messages.slice(1).map(m => m.message.content);
  assert.equal(fired.length, 1);
  assert.match(fired[0], /task t-c2 · matched/);
  assert.match(fired[0], /review on descendant t-b1/);
  h.handlers.session_shutdown({}, h.ctx);
});

test('a new wake replays from its own cursor, whatever other wakes already saw', async () => {
  const {tasks, h} = await started();
  tasks.history.push(event('t-c2', 'claimed'));
  await taskWake(h, {until: ['claimed'], limit: 3});
  // An event lands after the new wake reads its head but before its follower
  // starts, and an earlier wake's follower has already delivered it.
  const head = tasks.port.head;
  tasks.port.head = async () => {
    const value = await head();
    tasks.emit(event('t-b1', 'review'));
    await settle();
    return value;
  };
  await taskWake(h, {until: ['review']});
  tasks.port.head = head;
  const [, follower] = tasks.live();
  assert.equal(follower.since(), seq - 1);
  // The follower replays the backlog after its cursor once it spawns, then
  // any later events in order.
  tasks.reconnect();
  tasks.emit(event('t-b1', 'claimed'));
  await settle();
  const reviews = h.messages.filter(m =>
    /review on descendant t-b1/.test(m.message.content),
  );
  assert.equal(reviews.length, 1, 'once, not skipped or repeated');
  h.handlers.session_shutdown({}, h.ctx);
});

test('wake_run looks the task up and fires with its status', async () => {
  const {tasks, h} = await started();
  const {details} = await taskWake(h, {limit: 2});
  tasks.tree.get('t-a0').status = 'review';
  assert.match(await h.text('wake_run', {id: details.id}), /fired \(1 total\)/);
  await settle();
  const {content, details: fired} = h.messages[0].message;
  assert.match(content, / · manual \(fire 1 of 2\)\n/);
  assert.match(content, /observed: current state/);
  assert.match(content, /status now: "review"/);
  assert.equal(fired.manual, true);
  tasks.fail('dots: catalog unavailable');
  await assert.rejects(
    h.call('wake_run', {id: details.id}),
    /could not read task t-a0 for k-[a-f0-9]+; it keeps waiting: dots: catalog unavailable/,
  );
  h.handlers.session_shutdown({}, h.ctx);
});

test('shutdown flushes cursors and waits for the task store follower to stop', async () => {
  const {tasks, h} = await started();
  await taskWake(h);
  tasks.emit(event('t-b1', 'claimed'));
  await settle();
  let stopped;
  tasks.closeAfter(
    new Promise(resolve => {
      stopped = resolve;
    }),
  );
  let finished = false;
  const shutdown = h.handlers.session_shutdown({}, h.ctx).then(() => {
    finished = true;
  });
  await settle();
  assert.equal(tasks.live().length, 0, 'unsubscribed at once');
  assert.equal(tasks.closed(), 1);
  assert.equal(records(h).at(-1).trigger.cursor, seq);
  assert.equal(finished, false);
  stopped();
  await shutdown;
  assert.equal(finished, true);
});

test('tool parameters constrain task values in the schema', async () => {
  const {h} = await started();
  const params = h.tools.get('wake').parameters.properties;
  assert.deepEqual(params.task.required, ['id']);
  assert.equal(params.task.additionalProperties, false);
  assert.equal(params.task.properties.id.pattern, '^t-[a-f0-9]+$');
  h.handlers.session_shutdown({}, h.ctx);
});
