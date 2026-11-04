import assert from 'node:assert/strict';
import test from 'node:test';
import register from '../extensions/status/index.ts';
import {
  childRecords,
  formatClear,
  formatRecord,
  rootRecord,
} from '../extensions/status/model.ts';
import {
  ACTIVITY_EVENT,
  publishActivity,
  recordId,
} from '../src/host/activity.ts';
import {summaryLine} from '../src/host/summary.ts';

const idle = {
  running: false,
  compacting: false,
  rest: {state: 'idle'},
  activities: new Map(),
  sessionName: 'docs',
};
const worker = {id: 'worker/review', title: 'review', message: 'reading files'};
const b64 = text => Buffer.from(text, 'utf8').toString('base64');

test('background work keeps the root working after the run settles', () => {
  assert.deepEqual(rootRecord({...idle, running: true}), {
    state: 'working',
    message: 'docs',
  });
  const settled = {...idle, rest: {state: 'done'}};
  assert.deepEqual(rootRecord(settled), {
    state: 'done',
    message: 'Ready for your next instruction',
  });
  const busy = {
    ...settled,
    activities: new Map([
      ['workers', [worker]],
      [
        'jobs',
        [
          {id: 'job/j-1', title: 'cargo test'},
          {id: 'job/j-2', title: 'npm test'},
        ],
      ],
    ]),
  };
  assert.deepEqual(rootRecord(busy), {
    state: 'working',
    message: '1 worker \u00b7 2 jobs',
  });
  assert.deepEqual(
    rootRecord({...idle, rest: {state: 'error', message: 'rate limited'}}),
    {state: 'error', message: 'rate limited'},
  );
});

test('an open dialog blocks; confirm asks permission, others a question', () => {
  const dialog = {kind: 'confirm', title: 'Delete branch?'};
  assert.deepEqual(rootRecord({...idle, running: true, dialog}), {
    state: 'blocked',
    kind: 'permission',
    message: 'Delete branch?',
  });
  assert.deepEqual(
    rootRecord({...idle, dialog: {kind: 'select', title: 'Pick a model'}}),
    {state: 'blocked', kind: 'question', message: 'Pick a model'},
  );
  assert.deepEqual(rootRecord({...idle, compacting: true}), {
    state: 'working',
    message: 'Compacting context',
  });
});

test('each live item is a working child record', () => {
  assert.deepEqual(
    childRecords({...idle, activities: new Map([['workers', [worker]]])}),
    [
      {
        state: 'working',
        id: 'worker/review',
        title: 'review',
        message: 'reading files',
      },
    ],
  );
});

test("reports follow the protocol's syntax and limits", () => {
  assert.equal(
    formatRecord({state: 'done', message: 'docs'}),
    `\x1b]7501;state=done:app=pi:msg=${b64('docs')}\x1b\\`,
  );
  assert.equal(
    formatRecord({
      state: 'blocked',
      kind: 'question',
      message: 'which\nbranch?',
    }),
    `\x1b]7501;state=blocked:app=pi:kind=question:msg=${b64('which branch?')}\x1b\\`,
  );
  // Children inherit `app` from the root record.
  assert.equal(
    formatRecord({state: 'working', id: 'job/j-1', title: 'x'.repeat(300)}),
    `\x1b]7501;state=working:id=job/j-1:title=${b64('x'.repeat(192))}\x1b\\`,
  );
  assert.equal(
    formatRecord({
      state: 'working',
      id: 'job/j-1',
      title: '\u00e9'.repeat(100),
    }).includes(b64('\u00e9'.repeat(96))),
    true,
  );
  assert.equal(
    formatRecord({state: 'idle', message: '  '}),
    '\x1b]7501;state=idle:app=pi\x1b\\',
  );
  assert.equal(formatClear(), '\x1b]7501;state=clear\x1b\\');
  assert.equal(
    formatClear('job/j-1'),
    '\x1b]7501;state=clear:id=job/j-1\x1b\\',
  );
});

test("record ids keep to the protocol's alphabet", () => {
  assert.equal(
    recordId('worker', 'review: auth/login'),
    'worker/review-auth-login',
  );
  assert.equal(recordId('job', '\u2603'), 'job/x');
  assert.equal(
    recordId('worker', 'a'.repeat(40)).length,
    'worker/'.length + 32,
  );
});

function host({
  mode = 'tui',
  tty = true,
  name = 'docs',
  branch = [],
  workerId,
} = {}) {
  const handlers = new Map();
  const listeners = new Map();
  const events = {
    on: (channel, handler) =>
      listeners.set(channel, [...(listeners.get(channel) ?? []), handler]),
    emit: (channel, data) => {
      for (const handler of listeners.get(channel) ?? []) handler(data);
    },
  };
  const pi = {
    on: (name, handler) => handlers.set(name, handler),
    events,
    getSessionName: () => name ?? undefined,
  };
  const writes = [];
  const env = workerId ? {DOTS_WORKER_ID: workerId} : {};
  let timers = [];
  const later = (run, ms) => {
    const timer = {run, ms};
    timers.push(timer);
    return () => {
      timers = timers.filter(t => t !== timer);
    };
  };
  /** Lets every pending settle period elapse. */
  const settle = () => {
    const due = timers;
    timers = [];
    for (const timer of due) timer.run();
  };
  register(
    pi,
    env,
    data => writes.push(data),
    () => tty,
    later,
  );
  const fire = (name, event = {}) =>
    handlers.get(name)?.(
      {type: name, ...event},
      {mode, sessionManager: {getBranch: () => branch}},
    );
  fire('session_start', {reason: 'startup'});
  return {fire, events, writes, env, settle, pending: () => timers.length};
}

const assistant = (stopReason = 'stop', errorMessage, text) => ({
  message: {
    role: 'assistant',
    stopReason,
    errorMessage,
    content: text === undefined ? [] : [{type: 'text', text}],
  },
});

test('worker completion stays quiet until the parent handles its result and settles', () => {
  const {fire, events, writes, env, settle} = host();
  assert.equal(env.PI_PROGRAM_STATUS, '0');
  // Starting replaces whatever an earlier instance left.
  assert.deepEqual(writes, [formatRecord({state: 'idle'})]);
  writes.length = 0;
  fire('agent_start');
  publishActivity(events, 'workers', [worker]);
  fire('message_end', assistant());
  fire('agent_settled', {aborted: false});
  publishActivity(events, 'workers', [{...worker, message: 'editing'}]);
  publishActivity(events, 'workers', [{...worker, message: 'editing'}]);
  publishActivity(events, 'workers', []);
  settle();
  assert.ok(!writes.some(write => write.includes('state=done')));
  fire('agent_start');
  fire(
    'message_end',
    assistant('stop', undefined, 'Reviewed the worker result.'),
  );
  fire('agent_settled', {aborted: false});
  settle();
  assert.deepEqual(writes, [
    formatRecord({state: 'working', message: 'docs'}),
    formatRecord({
      state: 'working',
      id: 'worker/review',
      title: 'review',
      message: 'reading files',
    }),
    formatRecord({state: 'working', message: '1 worker'}),
    formatRecord({
      state: 'working',
      id: 'worker/review',
      title: 'review',
      message: 'editing',
    }),
    formatClear('worker/review'),
    formatRecord({state: 'idle'}),
    formatRecord({state: 'working', message: 'docs'}),
    formatRecord({state: 'done', message: 'Reviewed the worker result.'}),
  ]);
  assert.equal(writes.filter(w => w.includes('state=done')).length, 1);
});

test('errors, aborts, and prompts', () => {
  const {fire, events, writes, settle} = host();
  fire('agent_start');
  fire('message_end', assistant('error', '429 Too Many Requests\nretry later'));
  fire('agent_settled', {aborted: false});
  settle();
  assert.equal(
    writes.at(-1),
    formatRecord({state: 'error', message: '429 Too Many Requests'}),
  );
  fire('agent_start');
  fire('ui_prompt_start', {kind: 'custom'});
  fire('ui_prompt_start', {kind: 'confirm', title: 'Allow?'});
  assert.equal(
    writes.at(-1),
    formatRecord({state: 'blocked', kind: 'permission', message: 'Allow?'}),
  );
  fire('ui_prompt_end', {kind: 'confirm'});
  assert.equal(
    writes.at(-1),
    formatRecord({state: 'working', message: 'docs'}),
  );
  fire('agent_settled', {aborted: true});
  settle();
  assert.equal(writes.at(-1), formatRecord({state: 'idle'}));
});

test('compaction reports working and ends however it ends', () => {
  const {fire, writes, settle} = host();
  fire('session_before_compact', {reason: 'manual'});
  assert.equal(
    writes.at(-1),
    formatRecord({state: 'working', message: 'Compacting context'}),
  );
  fire('session_compact', {reason: 'manual'});
  settle();
  assert.equal(writes.at(-1), formatRecord({state: 'idle'}));
  fire('session_before_compact', {reason: 'manual'});
  fire('session_compact_failed', {reason: 'manual', aborted: true});
  settle();
  assert.equal(writes.at(-1), formatRecord({state: 'idle'}));
  fire('session_before_compact', {reason: 'threshold'});
  fire('session_compact_failed', {
    reason: 'threshold',
    aborted: false,
    errorMessage: 'summary failed',
  });
  settle();
  assert.equal(
    writes.at(-1),
    formatRecord({state: 'error', message: 'summary failed'}),
  );
});

test('finished work that wakes the agent never flashes done', () => {
  const {fire, events, writes, settle} = host();
  fire('agent_start');
  publishActivity(events, 'jobs', [{id: 'job/j-1', title: 'sleep 25'}]);
  fire('agent_settled', {aborted: false});
  publishActivity(events, 'jobs', []);
  // The result wakes the agent before the settle period elapses.
  fire('agent_start');
  settle();
  fire('agent_settled', {aborted: false});
  settle();
  assert.equal(writes.filter(w => w.includes('state=done')).length, 1);
  assert.equal(
    writes.at(-1),
    formatRecord({state: 'done', message: 'Ready for your next instruction'}),
  );
});

test('quitting clears every record; a reload reports nothing the next instance would contradict', () => {
  // The next instance reports the current state when it starts; flushing "done" first would be a false notification.
  const reload = host();
  reload.fire('agent_start');
  publishActivity(reload.events, 'workers', [worker]);
  reload.fire('agent_settled', {aborted: false});
  publishActivity(reload.events, 'workers', []);
  const before = reload.writes.length;
  reload.fire('session_shutdown', {reason: 'reload'});
  assert.equal(reload.pending(), 0);
  assert.equal(reload.writes.length, before);
  assert.ok(!reload.writes.some(w => w.includes('state=done')));
  const quit = host();
  quit.fire('session_shutdown', {reason: 'quit'});
  assert.deepEqual(quit.writes.slice(-1), [formatClear()]);
});

test('nothing is written outside an interactive terminal', () => {
  for (const options of [{mode: 'rpc'}, {tty: false}, {workerId: 'w-child'}]) {
    const {fire, events, writes} = host(options);
    fire('agent_start');
    events.emit(ACTIVITY_EVENT, {
      source: 'jobs',
      items: [{id: 'job/j-1', title: 'make'}],
    });
    fire('session_shutdown', {reason: 'quit'});
    assert.deepEqual(writes, []);
  }
});

test('without a session name, the latest prompt says what the session is about', () => {
  const unnamed = {...idle, sessionName: undefined, prompt: 'fix the footer'};
  assert.deepEqual(rootRecord({...unnamed, running: true}), {
    state: 'working',
    message: 'fix the footer',
  });
  assert.deepEqual(
    rootRecord({...unnamed, rest: {state: 'done', message: 'Footer fixed.'}}),
    {
      state: 'done',
      message: 'Footer fixed.',
    },
  );
  // A name wins over the prompt.
  assert.deepEqual(
    rootRecord({...unnamed, sessionName: 'docs', running: true}),
    {
      state: 'working',
      message: 'docs',
    },
  );
});

test('completion uses the final assistant answer, not the prompt or session name', () => {
  const {fire, writes, settle} = host();
  fire('before_agent_start', {
    messageType: 'user',
    prompt: 'Please fix the parser.',
  });
  fire('agent_start');
  fire('message_end', assistant('error', 'transient failure'));
  fire('agent_start');
  fire('message_end', {
    message: {
      role: 'assistant',
      stopReason: 'stop',
      content: [
        {type: 'thinking', thinking: 'Private reasoning'},
        {
          type: 'text',
          text: '\nParser fixed; all checks pass.\nDetails follow.',
        },
      ],
    },
  });
  fire('agent_settled', {aborted: false});
  settle();
  assert.equal(
    writes.at(-1),
    formatRecord({state: 'done', message: 'Parser fixed; all checks pass.'}),
  );
  assert.ok(!writes.some(write => write.includes('state=error')));
  const before = writes.length;
  fire('session_info_changed');
  assert.equal(
    writes.length,
    before,
    'metadata refresh does not repeat a completion',
  );
});

test('a completion summarizes the answer, skipping Markdown structure', () => {
  const cases = [
    [
      'plain prose',
      'Parser fixed; all checks pass.\nDetails follow.',
      'Parser fixed; all checks pass.',
    ],
    ['list first', '1. **Parser** fixed\n2. Tests added', 'Parser fixed'],
    ['lone list numbers', '1\n\n2.\n- Footer aligned', 'Footer aligned'],
    [
      'lead-in and list',
      'Here is what changed:\n\n- Fixed the [parser](src/p.ts)',
      'Here is what changed: Fixed the parser',
    ],
    [
      'heading then list',
      '## Changes\n\n- [x] Renamed `foo`',
      'Changes: Renamed foo',
    ],
    [
      'heading then prose',
      '# Summary\n\nThe footer now aligns.',
      'The footer now aligns.',
    ],
    [
      'code first',
      '```ts\nconst x = 1;\n# not a heading\n```\n\nThat adds the constant.',
      'That adds the constant.',
    ],
    [
      'table and rule',
      '| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n> Both rows match.',
      'Both rows match.',
    ],
    ['only code', '```\nmake test\n```', undefined],
    ['empty', ' \n\n', undefined],
    ['missing', undefined, undefined],
  ];
  for (const [name, text, want] of cases)
    assert.equal(summaryLine(text), want, name);
  const long = `${'Word '.repeat(30).trim()}. ${'more '.repeat(40)}`;
  assert.equal(summaryLine(long), `${'Word '.repeat(30).trim()}.`);
  const unbroken = 'word '.repeat(60);
  const cut = summaryLine(unbroken);
  assert.ok(cut.length <= 200 && cut.endsWith('word\u2026'), cut);

  const {fire, writes, settle} = host();
  const finish = (state, text, errorMessage) => {
    fire('agent_start');
    fire('message_end', assistant(state, errorMessage, text));
    fire('agent_settled', {aborted: false});
    settle();
    return writes.at(-1);
  };
  assert.equal(
    finish('stop', '1. Parser fixed\n2. Tests added'),
    formatRecord({state: 'done', message: 'Parser fixed'}),
  );
  assert.equal(
    finish('stop', '```\nonly code\n```'),
    formatRecord({state: 'done', message: 'Ready for your next instruction'}),
  );
  assert.equal(
    finish('error', undefined, 'Request failed:\n  overloaded'),
    formatRecord({state: 'error', message: 'Request failed: overloaded'}),
  );
});

test('idle user dialogs do not advertise blocked or repeat completion', () => {
  const {fire, writes, settle} = host();
  fire('agent_start');
  fire('message_end', assistant('stop', undefined, 'Ready for review.'));
  fire('agent_settled', {aborted: false});
  settle();
  const before = writes.length;
  for (const kind of ['input', 'select', 'confirm', 'custom']) {
    fire('ui_prompt_start', {kind, title: 'A user-opened picker'});
    fire('ui_prompt_end', {kind});
  }
  settle();
  assert.equal(writes.length, before);
});

test('background work that starts while idle cannot resurrect a completed response', () => {
  for (const source of ['workers', 'jobs']) {
    const {fire, events, writes, settle} = host();
    fire('agent_start');
    fire('message_end', assistant('stop', undefined, 'Already finished.'));
    fire('agent_settled', {aborted: false});
    settle();
    writes.length = 0;
    publishActivity(events, source, [worker]);
    publishActivity(events, source, []);
    settle();
    assert.ok(!writes.some(write => write.includes('state=done')));
    assert.equal(writes.at(-1), formatRecord({state: 'idle'}));
  }
});

test('a prompt is reported by its first line, and a resumed session by its last one', () => {
  const branch = [
    {type: 'message', message: {role: 'user', content: 'old ask'}},
    {type: 'message', message: {role: 'assistant', content: []}},
    {
      type: 'message',
      message: {
        role: 'user',
        content: [{type: 'text', text: '\n  resume this\nmore'}],
      },
    },
  ];
  const {fire, writes} = host({name: null, branch});
  writes.length = 0;
  fire('agent_start');
  assert.deepEqual(writes, [
    formatRecord({state: 'working', message: 'resume this'}),
  ]);
  writes.length = 0;
  fire('agent_settled', {aborted: false});
  fire('before_agent_start', {
    messageType: 'user',
    prompt: 'polish Go rows\ndetails',
  });
  fire('agent_start');
  assert.deepEqual(
    writes.at(-1),
    formatRecord({state: 'working', message: 'polish Go rows'}),
  );
  // A run a custom message starts keeps the user's prompt.
  fire('before_agent_start', {messageType: 'custom'});
  fire('agent_settled', {aborted: false});
  fire('agent_start');
  assert.deepEqual(
    writes.at(-1),
    formatRecord({state: 'working', message: 'polish Go rows'}),
  );
});
