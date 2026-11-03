import assert from 'node:assert/strict';
import test from 'node:test';
import {checkDigest} from '../extensions/wake/check.ts';
import {describe, parseDuration, shouldFire} from '../extensions/wake/index.ts';

import {fakeClock, harness} from './wake-fixture.mjs';

test('the TUI wake section follows creation, checking, queued consumption, cancellation and shutdown', async () => {
  const clock = fakeClock();
  let complete;
  const h = harness(
    clock,
    () =>
      new Promise(resolve => {
        complete = resolve;
      }),
    {mode: 'tui'},
  );
  await h.handlers.session_start({}, h.ctx);
  assert.equal(h.widgets.size, 0);
  const wake = await h.call('wake', {
    text: 'Inspect status',
    in: '2s',
    check: 'fixture-check',
  });
  assert.ok(h.widgets.has('dots.wakes'));
  const {initTheme} = await import('@earendil-works/pi-coding-agent');
  const {getThemeByName} =
    await import('../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js');
  const {stripTerminalSequences} = await import('@earendil-works/pi-tui');
  initTheme('dark', false);
  let renders = 0;
  const view = h.widgets.get('dots.wakes')(
    {requestRender: () => renders++},
    getThemeByName('dark'),
  );
  const shown = () => stripTerminalSequences(view.render(120).join('\n'));
  assert.match(shown(), /in 2s/);
  await clock.advance(1000);
  assert.match(shown(), /in 1s/);
  assert.ok(renders > 0);
  await clock.advance(1000);
  assert.match(shown(), /checking/);
  h.busy();
  complete({stdout: 'changed', stderr: '', code: 0});
  await new Promise(resolve => setImmediate(resolve));
  assert.match(shown(), /queued/);
  assert.equal(h.messages.length, 0, 'a fire waits for the run to settle');
  await h.turn();
  assert.equal(h.messages.length, 1);
  assert.equal(
    h.widgets.size,
    0,
    'a fired one-shot disappears only once delivered',
  );
  assert.equal(clock.pending(), 0, 'the display clock stops when empty');
  const recurring = await h.call('wake', {text: 'Poll status', every: '1m'});
  await h.call('wake_cancel', {id: recurring.id});
  assert.equal(h.widgets.size, 0);
  await h.call('wake', {text: 'Later', in: '5m'});
  await h.handlers.session_shutdown({}, h.ctx);
  assert.equal(h.widgets.size, 0);
  assert.equal(clock.pending(), 0);
  assert.equal((await h.call('wake_run', {id: wake.id})).active, false);
});

test('durations and gating helpers', () => {
  assert.equal(parseDuration('20m'), 1_200_000);
  assert.equal(parseDuration('1.5h'), 5_400_000);
  assert.throws(() => parseDuration('soon'), /duration/);
  assert.equal(describe(90_000), '2m');
  assert.equal(describe(7_200_000), '2h');
  const base = {on: 'change', lastDigest: checkDigest('a', ''), lastCode: 0};
  assert.equal(shouldFire(base, {stdout: 'a', stderr: '', code: 0}), false);
  assert.equal(shouldFire(base, {stdout: 'b', stderr: '', code: 0}), true);
  assert.equal(
    shouldFire({on: 'change'}, {stdout: 'a', stderr: '', code: 0}),
    true,
    'first observation fires',
  );
  assert.equal(
    shouldFire({on: 'failure'}, {stdout: '', stderr: '', code: 1}),
    true,
  );
  assert.equal(
    shouldFire({on: 'failure'}, {stdout: '', stderr: '', code: 0}),
    false,
  );
  assert.equal(
    shouldFire({on: 'success'}, {stdout: '', stderr: '', code: 0}),
    true,
  );
});

test('one-shot, recurring with limit, no drift, no pile-up, cancel, manual run', async () => {
  const clock = fakeClock();
  const h = harness(clock);
  h.handlers.session_start({reason: 'startup'}, h.ctx);

  const once = await h.call('wake', {text: 'check the deploy', in: '20m'});
  assert.equal(once.in, '20m');
  await clock.advance(19 * 60_000);
  assert.equal(h.messages.length, 0);
  await clock.advance(60_000);
  assert.equal(h.messages.length, 1);
  assert.match(h.messages[0].message.content, /check the deploy/);
  assert.deepEqual(h.messages[0].options, {triggerTurn: true});
  assert.equal(
    (await h.call('wake_list', {})).length,
    0,
    'one-shot removes itself',
  );

  const every = await h.call('wake', {text: 'poll', every: '10m', limit: 3});
  assert.equal(every.every, '10m');
  await clock.advance(10 * 60_000);
  assert.equal(h.messages.length, 2);
  assert.match(h.messages[1].message.content, /fire 1 of 3/);
  // A busy session holds the next fire; a tick while it waits is skipped, not
  // stacked.
  h.busy();
  await clock.advance(10 * 60_000);
  await clock.advance(10 * 60_000);
  assert.equal(h.messages.length, 2);
  await h.turn();
  assert.equal(h.messages.length, 3);
  assert.match(h.messages[2].message.content, /fire 2 of 3/);
  await clock.advance(10 * 60_000);
  assert.equal(h.messages.length, 4);
  assert.match(h.messages[3].message.content, /fire 3 of 3/);
  await clock.advance(30 * 60_000);
  assert.equal(h.messages.length, 4, 'limit reached');
  assert.equal((await h.call('wake_list', {})).length, 0);

  const loop = await h.call('wake', {text: 'tick', every: '5m'});
  assert.equal(loop.limit, 50);
  await clock.advance(5 * 60_000);
  assert.equal(h.messages.length, 5);
  assert.match(await h.text('wake_run', {id: loop.id}), /fired \(2 total\)/);
  assert.match(h.messages[5].message.content, /manual/);
  assert.match(
    await h.text('wake_cancel', {id: loop.id.slice(0, 10)}),
    /cancelled after 2/,
  );
  await clock.advance(60 * 60_000);
  assert.equal(h.messages.length, 6);

  await assert.rejects(h.call('wake', {text: 'x', every: '10s'}), /at least/);
  await assert.rejects(
    h.call('wake', {text: 'x', in: '1m', every: '5m'}),
    /exactly one/,
  );
  await assert.rejects(
    h.call('wake', {text: 'x', at: '2001-01-01T00:00:00Z'}),
    /past/,
  );
  await assert.rejects(
    h.call('wake', {text: 'x', in: '5m', on: 'failure'}),
    /requires check/,
  );
  await assert.rejects(
    h.call('wake', {text: 'x', in: '5m', limit: 3}),
    /limit applies only to every/,
  );
  assert.equal(clock.pending(), 0, 'no timers left behind');
});

test('check-gated wakes only wake on the chosen condition and carry output', async () => {
  const clock = fakeClock();
  const runs = [];
  let output = {stdout: 'pending\n', stderr: '', code: 0};
  const h = harness(clock, async (command, cwd) => {
    runs.push([command, cwd]);
    return output;
  });
  h.handlers.session_start({reason: 'startup'}, h.ctx);

  const ci = await h.call('wake', {
    text: 'CI changed; inspect',
    every: '5m',
    check: 'gh run list --limit 1',
    on: 'change',
  });
  await clock.advance(5 * 60_000);
  assert.equal(runs.length, 1);
  assert.deepEqual(runs[0], ['gh run list --limit 1', '/project']);
  assert.equal(h.messages.length, 1, 'first observation fires');
  assert.match(h.messages[0].message.content, /exit: 0\npending/);
  await clock.advance(5 * 60_000);
  assert.equal(h.messages.length, 1, 'same output: silent');
  output = {stdout: 'failure\n', stderr: '', code: 0};
  await clock.advance(5 * 60_000);
  assert.equal(h.messages.length, 2, 'changed output fires');
  assert.match(h.messages[1].message.content, /failure/);

  const red = await h.call('wake', {
    text: 'build broke',
    every: '2m',
    check: 'make check',
    on: 'failure',
  });
  output = {stdout: 'ok', stderr: '', code: 0};
  await clock.advance(2 * 60_000);
  assert.equal(h.messages.length, 2, 'green build: silent');
  output = {stdout: '', stderr: 'boom', code: 2};
  await clock.advance(2 * 60_000);
  assert.equal(h.messages.length, 3);
  assert.match(h.messages[2].message.content, /exit: 2\nstderr: boom/);
  assert.match(await h.text('wake_run', {id: red.id}), /fired/);
  assert.equal(h.messages.length, 4, 'manual run ignores the gate');
  await h.call('wake_cancel', {id: ci.id});
  await h.call('wake_cancel', {id: red.id});
});

test('reload re-arms from the session; a missed one-shot fires once, late', async () => {
  const clock = fakeClock();
  const h = harness(clock);
  h.handlers.session_start({reason: 'startup'}, h.ctx);
  const soon = await h.call('wake', {text: 'soon', in: '5m'});
  const later = await h.call('wake', {text: 'later', in: '60m'});
  const cadence = await h.call('wake', {
    text: 'cadence',
    every: '15m',
    limit: 10,
  });
  h.handlers.session_shutdown({}, h.ctx);
  assert.equal(clock.pending(), 0);

  // Away for 40 minutes, then reload the same session.
  await clock.advance(40 * 60_000);
  const h2 = harness(clock);
  h2.entries.push(...h.entries);
  h2.handlers.session_start({reason: 'reload'}, h2.ctx);
  await clock.advance(0);
  const fired = h2.messages.map(m => m.message.content);
  assert.equal(
    fired.filter(c => /\bsoon\b/.test(c)).length,
    1,
    'missed one-shot fires once',
  );
  assert.match(
    fired.find(c => /\bsoon\b/.test(c)),
    /late by 35m/,
  );
  assert.equal(
    fired.filter(c => /\bcadence\b/.test(c)).length,
    1,
    'missed recurring collapses to one fire',
  );
  assert.equal(fired.filter(c => /\blater\b/.test(c)).length, 0);
  const listed = await h2.call('wake_list', {});
  assert.deepEqual(listed.map(w => w.id).sort(), [cadence.id, later.id].sort());
  assert.equal(
    listed.find(w => w.id === cadence.id).in,
    '15m',
    'continues from now, not from the missed slots',
  );
  await clock.advance(20 * 60_000);
  assert.equal(
    h2.messages.filter(m => /\blater\b/.test(m.message.content)).length,
    1,
  );
  assert.equal(
    h2.messages.filter(m => /\bcadence\b/.test(m.message.content)).length,
    2,
  );
  h2.handlers.session_shutdown({}, h2.ctx);
  assert.equal(clock.pending(), 0);
});

test('in-flight checks cannot publish or re-arm after cancellation or shutdown', async () => {
  for (const action of ['cancel', 'shutdown', 'replace']) {
    const clock = fakeClock();
    let complete;
    const h = harness(
      clock,
      () =>
        new Promise(resolve => {
          complete = resolve;
        }),
    );
    h.handlers.session_start({}, h.ctx);
    const wake = await h.call('wake', {
      text: 'check',
      every: '1m',
      check: 'true',
    });
    await clock.advance(60_000);
    assert.equal(typeof complete, 'function');
    if (action === 'cancel') await h.call('wake_cancel', {id: wake.id});
    else h.handlers.session_shutdown({}, h.ctx);
    if (action === 'replace') {
      h.entries.length = 0;
      h.handlers.session_start({}, h.ctx);
    }
    const saved = h.entries.length;
    complete({stdout: 'done', stderr: '', code: 0});
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.messages.length, 0, action);
    assert.equal(h.entries.length, saved, action);
    assert.equal(clock.pending(), 0, action);
  }
});

test('a gated missed wake advances on restore instead of disappearing', async () => {
  const clock = fakeClock();
  const run = async () => ({stdout: 'same', stderr: '', code: 0});
  const h = harness(clock, run);
  h.handlers.session_start({}, h.ctx);
  await h.call('wake', {text: 'poll', every: '1m', check: 'true'});
  await clock.advance(60_000);
  h.handlers.session_shutdown({}, h.ctx);
  await clock.advance(120_000);
  const restored = harness(clock, run);
  restored.entries.push(...h.entries);
  restored.handlers.session_start({}, restored.ctx);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(restored.messages.length, 0);
  assert.equal(clock.pending(), 1);
  assert.equal((await restored.call('wake_list', {}))[0].in, '1m');
  restored.handlers.session_shutdown({}, restored.ctx);
});

test("30-day wakes chunk timers within Node's signed-int32 delay limit", async () => {
  const clock = fakeClock();
  const setTimeout = clock.setTimeout;
  clock.setTimeout = (fn, ms) => {
    assert.ok(ms <= 2_147_483_647);
    return setTimeout(fn, ms);
  };
  const h = harness(clock);
  h.handlers.session_start({}, h.ctx);
  await h.call('wake', {text: 'later', in: '30d'});
  await clock.advance(2_147_483_647);
  assert.equal(h.messages.length, 0);
  assert.equal(clock.pending(), 1);
  await clock.advance(30 * 86400_000 - 2_147_483_647);
  assert.equal(h.messages.length, 1);
});

test('manual checks own the timer, reject overlap, and enforce the shared limit', async () => {
  const clock = fakeClock();
  let complete;
  let runs = 0;
  const h = harness(clock, () => {
    runs++;
    return new Promise(resolve => {
      complete = resolve;
    });
  });
  await h.handlers.session_start({}, h.ctx);
  const wake = await h.call('wake', {
    text: 'poll',
    every: '1m',
    limit: 2,
    check: 'true',
  });
  await clock.advance(30_000);
  // wake_run is a tool call: the run that made it is in progress.
  h.busy();
  const manual = h.text('wake_run', {id: wake.id});
  await assert.rejects(h.call('wake_run', {id: wake.id}), /check in progress/);
  await clock.advance(60_000);
  assert.equal(runs, 1, 'the scheduled slot cannot overlap the manual check');
  complete({stdout: 'ok', stderr: '', code: 0});
  assert.match(await manual, /fired \(1 total\)/);
  await clock.advance(0);
  await h.turn();
  assert.equal(
    h.messages.length,
    1,
    'the missed scheduled slot is not stacked',
  );
  assert.equal((await h.call('wake_list', {}))[0].in, '30s');
  const final = h.text('wake_run', {id: wake.id});
  complete({stdout: 'ok', stderr: '', code: 0});
  assert.match(await final, /fired \(2 total\)/);
  assert.equal(clock.pending(), 0);
  assert.equal((await h.call('wake_list', {})).length, 0);
  assert.match(await h.text('wake_run', {id: wake.id}), /did not fire/);
  assert.equal(runs, 2);
});

test('held fires share one run and release their slot once delivered', async () => {
  const clock = fakeClock();
  const h = harness(clock);
  await h.handlers.session_start({}, h.ctx);
  const wake = await h.call('wake', {text: 'poll', every: '1m', limit: 4});
  h.busy();
  await clock.advance(60_000);
  await h.call('wake_run', {id: wake.id});
  await clock.advance(60_000);
  assert.equal(h.messages.length, 0, 'fires wait while the session is busy');
  await h.turn();
  assert.deepEqual(
    h.messages.map(({message, options}) => [
      message.details.fired,
      options.triggerTurn,
    ]),
    [
      [1, false],
      [2, true],
    ],
    'appended together; the last starts the run',
  );
  await clock.advance(60_000);
  assert.equal(h.messages.length, 3, 'delivery releases the pending slot');
  h.handlers.session_shutdown({}, h.ctx);
});

test('a fire held through /tree carries the provenance of the branch it lands on', async () => {
  const clock = fakeClock();
  const h = harness(clock);
  await h.handlers.session_start({}, h.ctx);
  await h.call('wake', {text: 'poll', in: '1m'});
  // A summarized navigation keeps the session busy while the fire is due.
  h.busy();
  await clock.advance(60_000);
  assert.equal(h.messages.length, 0);
  h.navigate();
  await h.turn();
  const [{message}] = h.messages;
  assert.equal(message.details.elsewhere, true);
  assert.match(message.content, / · set on another branch of this session\n/);
  h.handlers.session_shutdown({}, h.ctx);
});

test('a fire after a cancelled run is recorded without a run until the user prompts', async () => {
  const clock = fakeClock();
  const h = harness(clock);
  await h.handlers.session_start({}, h.ctx);
  await h.call('wake', {text: 'poll', every: '1m', limit: 4});
  h.busy();
  await clock.advance(60_000);
  await h.turn({aborted: true});
  assert.deepEqual(h.messages.at(-1)?.options, {triggerTurn: false});
  await clock.advance(60_000);
  assert.equal(h.messages.length, 2, 'the next fire is not skipped');
  assert.deepEqual(h.messages.at(-1)?.options, {triggerTurn: false});
  h.prompt();
  await clock.advance(60_000);
  assert.deepEqual(h.messages.at(-1)?.options, {triggerTurn: true});
  h.handlers.session_shutdown({}, h.ctx);
});

test('checks persist a bounded digest rather than complete output', async () => {
  const clock = fakeClock();
  const h = harness(clock, async () => ({
    stdout: 'x'.repeat(200_000),
    stderr: '',
    code: 0,
  }));
  await h.handlers.session_start({}, h.ctx);
  await h.call('wake', {text: 'poll', every: '1m', check: 'verbose fixture'});
  await clock.advance(60_000);
  await clock.advance(60_000);
  assert.equal(h.messages.length, 1);
  assert.ok(h.entries.every(entry => JSON.stringify(entry).length < 1000));
  assert.match(
    h.entries.at(-1).data.trigger.check.lastDigest,
    /^[0-9a-f]{64}:[0-9a-f]{64}:$/,
  );
  h.handlers.session_shutdown({}, h.ctx);
});

test('cancelling a manual check aborts its runner and does not claim a fire', async () => {
  const clock = fakeClock();
  let complete;
  let signal;
  const h = harness(clock, (_command, _cwd, passed) => {
    signal = passed;
    return new Promise(resolve => {
      complete = resolve;
    });
  });
  await h.handlers.session_start({}, h.ctx);
  const wake = await h.call('wake', {text: 'later', in: '1m', check: 'true'});
  const pending = h.text('wake_run', {id: wake.id});
  await h.call('wake_cancel', {id: wake.id});
  assert.equal(signal.aborted, true);
  complete({stdout: 'ignored', stderr: '', code: 0});
  assert.match(await pending, /did not fire/);
  assert.equal(h.messages.length, 0);
  assert.equal(clock.pending(), 0);
});

test('wakes are session resources: /tree keeps them armed and reload agrees', async () => {
  const clock = fakeClock();
  const h = harness(clock);
  h.handlers.session_start({reason: 'startup'}, h.ctx);
  const poll = await h.call('wake', {text: 'poll CI', every: '10m', limit: 5});
  h.navigate();
  assert.equal(
    (await h.call('wake_list', {})).length,
    1,
    'navigation neither cancels nor forgets',
  );
  await clock.advance(10 * 60_000);
  assert.equal(h.messages.length, 1, 'the wake fires into the current branch');
  assert.match(
    h.messages[0].message.content,
    /^Wake k-[0-9a-f]{16} \(fire 1 of 5, every 10m\) · set on another branch of this session\n/,
  );
  assert.equal(h.messages[0].message.details.elsewhere, true);
  h.handlers.session_shutdown({}, h.ctx);
  // Reload from the same session file, still on the new branch: the same wake, the same schedule.
  const reloaded = harness(clock);
  reloaded.entries.push(...h.entries);
  reloaded.handlers.session_start({reason: 'reload'}, reloaded.ctx);
  const listed = await reloaded.call('wake_list', {});
  assert.deepEqual(
    listed.map(w => [w.id, w.fired, w.in]),
    [[poll.id, 1, '10m']],
  );
  reloaded.handlers.session_shutdown({}, reloaded.ctx);
  assert.equal(clock.pending(), 0);
});

test('a fork or clone inherits wake records as history and never fires them', async () => {
  const clock = fakeClock();
  const parent = harness(clock);
  parent.handlers.session_start({reason: 'startup'}, parent.ctx);
  await parent.call('wake', {text: 'parent follow-up', in: '5m'});
  parent.handlers.session_shutdown({}, parent.ctx);
  const fork = harness(clock, undefined, {session: 'session-fork'});
  fork.entries.push(...parent.entries);
  fork.handlers.session_start({reason: 'fork'}, fork.ctx);
  assert.deepEqual(await fork.call('wake_list', {}), []);
  await assert.rejects(fork.call('wake_cancel', {id: 'k-'}), /no wake/);
  await clock.advance(10 * 60_000);
  assert.equal(fork.messages.length, 0);
  assert.equal(clock.pending(), 0);
  fork.handlers.session_shutdown({}, fork.ctx);
  // Resuming the original session keeps ownership.
  const resumed = harness(clock);
  resumed.entries.push(...parent.entries);
  resumed.handlers.session_start({reason: 'resume'}, resumed.ctx);
  await clock.advance(0);
  assert.equal(
    resumed.messages.length,
    1,
    "the owner's missed wake fires once on resume",
  );
  resumed.handlers.session_shutdown({}, resumed.ctx);
});

test('incompatible wake records are diagnosed and never armed; valid ones still restore', async () => {
  const clock = fakeClock();
  const h = harness(clock);
  h.handlers.session_start({}, h.ctx);
  const kept = await h.call('wake', {text: 'kept', in: '5m'});
  h.handlers.session_shutdown({}, h.ctx);
  const restored = harness(clock);
  restored.entries.push(
    {
      type: 'custom',
      id: 'legacy',
      customType: 'dots.wake',
      data: {
        id: 'k-0123456789abcdef',
        text: 'versionless',
        at: clock.now() + 60_000,
        limit: 1,
        fired: 0,
        on: 'change',
        createdAt: 0,
      },
    },
    {
      type: 'custom',
      id: 'wrong',
      customType: 'dots.wake',
      data: {
        version: 1,
        owner: 'session-a',
        id: 'k-fedcba9876543210',
        text: 'bad',
        at: -1,
        limit: 1,
        fired: 0,
        on: 'sometimes',
        cancelled: false,
        createdAt: 0,
      },
    },
    ...h.entries,
  );
  restored.handlers.session_start({reason: 'reload'}, restored.ctx);
  assert.deepEqual(
    (await restored.call('wake_list', {})).map(w => w.id),
    [kept.id],
  );
  assert.equal(restored.notices.length, 1);
  assert.match(
    restored.notices[0].message,
    /^2 wake records were incompatible and not restored \(/,
  );
  await clock.advance(60_000);
  assert.equal(
    restored.messages.length,
    0,
    'the versionless record never fires',
  );
  restored.handlers.session_shutdown({}, restored.ctx);
});

test('tool parameters constrain one-right-answer values in the schema', () => {
  const h = harness(fakeClock());
  const params = h.tools.get('wake').parameters.properties;
  assert.deepEqual(
    params.on.anyOf.map(s => s.const),
    ['always', 'change', 'failure', 'success'],
  );
  assert.equal(params.limit.type, 'integer');
  assert.equal(params.limit.minimum, 1);
  assert.equal(params.limit.maximum, 1000);
});

test('an unreadable latest record of a wake never gives an older timer back its authority', async () => {
  const clock = fakeClock();
  const runs = [];
  const h = harness(clock, async command => {
    runs.push(command);
    return {stdout: '', stderr: '', code: 0};
  });
  h.handlers.session_start({}, h.ctx);
  const wake = await h.call('wake', {
    text: 'check',
    in: '1s',
    check: 'fixture-check',
    on: 'always',
  });
  const kept = await h.call('wake', {text: 'kept', in: '5m'});
  h.handlers.session_shutdown({}, h.ctx);
  const record = h.entries.find(entry => entry.data.id === wake.id).data;
  h.entries.push({
    type: 'custom',
    id: 'broken',
    customType: 'dots.wake',
    data: {
      ...record,
      active: false,
      trigger: {
        ...record.trigger,
        check: {...record.trigger.check, on: 'invalid-policy'},
      },
    },
  });
  const reloaded = harness(clock, async command => {
    runs.push(command);
    return {stdout: '', stderr: '', code: 0};
  });
  reloaded.entries.push(...h.entries);
  reloaded.handlers.session_start({reason: 'reload'}, reloaded.ctx);
  assert.match(
    reloaded.notices[0].message,
    /^1 wake record was incompatible and not restored \(\/trigger\/check\/on /,
  );
  assert.deepEqual(
    (await reloaded.call('wake_list', {})).map(w => w.id),
    [kept.id],
  );
  await clock.advance(2_000);
  assert.deepEqual(runs, [], 'the older check never runs');
  assert.equal(reloaded.messages.length, 0);
  assert.equal(clock.pending(), 1, 'only the readable wake is armed');
  reloaded.handlers.session_shutdown({}, reloaded.ctx);
});
