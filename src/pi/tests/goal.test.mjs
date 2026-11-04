import assert from 'node:assert/strict';
import test, {beforeEach, afterEach} from 'node:test';
import {initTheme} from '@earendil-works/pi-coding-agent';
import {getThemeByName} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';
import goal, {
  FOCUS_ENTRY,
  restoreFocus,
  restoreGoals,
} from '../extensions/goal/index.ts';
import {source, snapshot} from './goal-fixture.mjs';
import {withHierarchy} from '../extensions/goal/source.ts';
import {WORKER_ACTIVITY_EVENT} from '../src/ui/worker-activity.ts';

// Tests own whether they simulate a worker; the launching agent does not.
const inheritedWorker = process.env.DOTS_WORKER_ID;
beforeEach(() => {
  delete process.env.DOTS_WORKER_ID;
});
afterEach(() => {
  if (inheritedWorker === undefined) delete process.env.DOTS_WORKER_ID;
  else process.env.DOTS_WORKER_ID = inheritedWorker;
});

const entry = focus => ({
  type: 'custom',
  customType: FOCUS_ENTRY,
  data: {version: 2, focus, followed: focus ? [focus] : []},
});
const focused = () => ({id: 't-123abc', project: '/example/project'});
function harness(backend = source(), saved = [], mode = 'rpc') {
  const handlers = new Map();
  const commands = new Map();
  const tools = new Map();
  const entries = [...saved];
  const notices = [];
  const messages = [];
  const userMessages = [];
  const widgets = [];
  const shortcuts = new Map();
  const bus = new Map();
  const pi = {
    events: {
      on(channel, handler) {
        bus.set(channel, handler);
        return () => bus.delete(channel);
      },
    },
    on(name, handler) {
      handlers.set(name, handler);
    },
    registerCommand(name, command) {
      commands.set(name, command);
    },
    registerTool(tool) {
      tools.set(tool.name, tool);
    },
    registerShortcut(key, shortcut) {
      shortcuts.set(key, shortcut);
    },
    sendUserMessage(message, options) {
      userMessages.push({message, options});
    },
    appendEntry(customType, data) {
      entries.push({type: 'custom', customType, data});
    },
    sendMessage(message, options) {
      messages.push({message, options});
    },
  };
  goal(pi, backend);
  const ctx = {
    cwd: '/example/linked',
    mode,
    hasUI: mode !== 'json',
    sessionManager: {
      getBranch: () => entries,
      getSessionId: () => 'fixture-session',
    },
    ui: {
      setStatus() {},
      notify: (...args) => notices.push(args),
      setWidget: (...args) => widgets.push(args),
      select: async () => undefined,
    },
  };
  return {
    backend,
    entries,
    notices,
    messages,
    userMessages,
    widgets,
    shortcuts,
    bus,
    ctx,
    tool: (id, signal, project) =>
      tools
        .get('goal_focus')
        .execute(
          'fixture',
          project ? {id, project} : {id},
          signal,
          undefined,
          ctx,
        ),
    unfollow: (args, signal) =>
      tools
        .get('goal_unfocus')
        .execute('fixture', args, signal, undefined, ctx),
    list: () =>
      tools.get('goal_list').execute('fixture', {}, undefined, undefined, ctx),
    event: (name, data = {}) => handlers.get(name)?.(data, ctx),
    command: args => commands.get('goal').handler(args, ctx),
    async context() {
      const event = {systemPromptOptions: {sections: {}}};
      await handlers.get('before_agent_start')(event, ctx);
      const contents = Object.values(event.systemPromptOptions.sections);
      return contents.length ? [{content: contents.join('\n\n')}] : [];
    },
  };
}
test('claimed goals are offers, not follows; dismissal survives resume and explicit unfollow', async () => {
  const backend = source();
  const owners = [];
  const list = backend.list;
  backend.list = async (cwd, signal, owner) => {
    owners.push(owner);
    return list(cwd, signal);
  };
  const h = harness(backend, [], 'tui');
  const offer = () =>
    h.widgets.findLast(([key]) => key === 'dots.goal.offers')?.[1];
  await h.event('session_start');
  assert.deepEqual(owners, ['pi-fixture-session']);
  assert.match(offer().join('\n'), /Follow: \/goal follow t-123abc/);
  assert.deepEqual((await h.list()).details.followed, []);
  assert.deepEqual(await h.context(), []);
  assert.equal(h.userMessages.length, 0);
  await h.command('dismiss t-123abc');
  assert.equal(offer(), undefined);
  await h.event('session_start');
  assert.equal(offer(), undefined);
  await h.tool('t-123abc');
  assert.equal((await h.list()).details.followed.length, 1);
  backend.list = async () => ({project: '/example/project', goals: []});
  await h.event('agent_end');
  assert.equal(
    (await h.list()).details.followed.length,
    1,
    'claim release does not unfollow',
  );
  backend.list = list;
  await h.unfollow({id: 't-123abc'});
  await h.event('session_start');
  assert.equal(offer(), undefined);
  assert.deepEqual((await h.list()).details.followed, []);
});

test('offers stay bounded, explicit listing reaches the rest, and unfollow suppresses a new offer', async () => {
  const backend = source();
  backend.list = async () => ({
    project: '/example/project',
    goals: ['t-111111', 't-222222', 't-333333', 't-444444'].map(
      id => snapshot(id).overview.goal,
    ),
  });
  const h = harness(backend, [], 'tui');
  await h.event('session_start');
  const lines = h.widgets.findLast(([key]) => key === 'dots.goal.offers')[1];
  assert.equal(lines.length, 4);
  assert.doesNotMatch(lines.join('\n'), /t-444444/);
  await h.command('offers');
  assert.match(h.notices.at(-1)[0], /t-444444/);
  await h.tool('t-444444');
  await h.unfollow({id: 't-444444'});
  await h.event('session_start');
  await h.command('offers');
  assert.doesNotMatch(h.notices.at(-1)[0], /t-444444/);
});

test('claim discovery cancels stale reads and reports failure without retaining offers', async () => {
  const backend = source();
  const h = harness(backend, [], 'tui');
  await h.event('session_start');
  const pending = gate();
  backend.list = () => pending.promise;
  const refresh = h.event('agent_end');
  await h.event('session_shutdown');
  const count = h.widgets.length;
  pending.resolve({
    project: '/example/project',
    goals: [snapshot().overview.goal],
  });
  await refresh;
  assert.equal(h.widgets.length, count);
  backend.list = async () => {
    throw new Error('fixture unavailable');
  };
  await h.event('session_start');
  await h.command('offers');
  assert.match(h.notices.at(-1)[0], /fixture unavailable/);
  assert.deepEqual((await h.list()).details.followed, []);
});

function gate() {
  let resolve;
  const promise = new Promise(r => {
    resolve = r;
  });
  return {promise, resolve};
}

test('new starts a planning conversation without writing tasks or changing focus', async () => {
  const h = harness();
  await h.event('session_start');
  await h.command('new Consistent exports across all formats');
  assert.equal(h.userMessages.length, 1);
  assert.match(
    h.userMessages[0].message,
    /Consistent exports across all formats/,
  );
  assert.match(h.userMessages[0].message, /dots task goal create/);
  assert.match(h.userMessages[0].message, /goal_focus/);
  assert.match(h.userMessages[0].message, /not to start implementation/);
  assert.equal(h.backend.calls.length, 0);
  assert.equal(h.entries.length, 0);
});

test('agent can focus after creation; cancelled tool reads cannot persist focus', async () => {
  const h = harness();
  await h.event('session_start');
  const result = await h.tool('t-123abc');
  assert.equal(result.details.id, 't-123abc');
  assert.match(result.content[0].text, /Use one shared export interface/);
  const pending = gate();
  const signal = new AbortController();
  h.backend.show = () => pending.promise;
  const change = h.tool('t-999999', signal.signal);
  signal.abort();
  pending.resolve(snapshot('t-999999'));
  await assert.rejects(change, /interrupted/);
  assert.equal(h.entries.at(-1).data.focus.id, 't-123abc');
  assert.equal(h.userMessages.length, 0);
});

test('headless inspection and agent orientation stay active-only', async () => {
  const backend = source();
  const h = harness(backend, [entry(focused())]);
  backend.show = async (_cwd, id) => {
    const value = snapshot(id);
    value.overview.progress.done = 1;
    value.overview.progress.total = 2;
    const old = {
      ...value.overview.tasks[0],
      id: 't-111111',
      title: 'Old finished slice',
      status: 'done',
    };
    return withHierarchy({...value, allTasks: [old, ...value.overview.tasks]});
  };
  await h.event('session_start');
  await h.command('show');
  assert.doesNotMatch(h.messages.at(-1).message.content, /Old finished slice/);
  assert.match(
    h.messages.at(-1).message.content,
    /1 closed task details hidden/,
  );
  assert.equal(h.messages.at(-1).options.triggerTurn, false);
  await h.command('history');
  assert.match(
    h.notices.at(-1)[0],
    /^\/goal \[new IDEA/,
    'history is a panel key',
  );
  const files = await h.context();
  assert.match(files[0].content, /Tasks: 1 open · 1 done/);
  assert.doesNotMatch(files[0].content, /Old finished slice/);
});

test('unfocused sessions do not query dots; focus persists identity without execution', async () => {
  const h = harness();
  await h.event('session_start');
  assert.deepEqual(await h.context(), []);
  assert.equal(h.backend.calls.length, 0);
  await h.command('focus t-123abc');
  assert.equal(h.entries.length, 1);
  assert.deepEqual(restoreGoals(h.entries), {
    focus: focused(),
    followed: [focused()],
  });
  const files = await h.context();
  assert.equal(files.length, 1);
  assert.match(files[0].content, /Use one shared export interface/);
  assert.match(files[0].content, /current user request takes precedence/);
  assert.match(files[0].content, /Reference slice/);
  await h.command('show');
  assert.equal(h.messages.length, 1);
  assert.equal(h.messages[0].options.triggerTurn, false);
  await h.command('unfocus');
  assert.equal(h.entries.at(-1).data.focus, null);
  assert.deepEqual(await h.context(), []);
  assert.ok(h.messages.every(m => m.options.triggerTurn === false));
});

test('cold resume and branch navigation restore focus but never resume work', async () => {
  const h = harness(source(), [entry(focused())]);
  await h.event('session_start');
  assert.equal(h.backend.calls.length, 1);
  assert.match((await h.context())[0].content, /Consistent exports/);
  h.entries.push(entry(null));
  await h.event('session_tree');
  assert.deepEqual(await h.context(), []);
  h.entries.pop();
  await h.event('session_tree');
  assert.match((await h.context())[0].content, /Consistent exports/);
  assert.equal(h.messages.length, 0);
  assert.equal(h.entries.length, 1);
});

test('refresh uses changed brief; unavailable data replaces old context, not a false success', async () => {
  const backend = source();
  const h = harness(backend, [entry(focused())]);
  await h.event('session_start');
  backend.briefContent = {text: 'Changed accepted pattern'};
  assert.match((await h.context())[0].content, /Changed accepted pattern/);
  backend.briefContent = {text: '', warning: 'Brief unavailable: removed'};
  assert.match((await h.context())[0].content, /Brief unavailable/);
  backend.show = async () => {
    throw new Error('dots unsupported');
  };
  const content = (await h.context())[0].content;
  assert.match(content, /Task state unavailable: Error: dots unsupported/);
  assert.doesNotMatch(
    content,
    /Changed accepted pattern|Use one shared export interface|1 ready/,
  );
});

test('a read answered for another project than recorded is unavailable', async () => {
  const backend = source();
  const h = harness(backend, [entry({...focused(), project: '/other'})]);
  await h.event('session_start');
  assert.equal(backend.calls.at(-1).project, '/other');
  assert.match((await h.context())[0].content, /different project/);
});

// Hand-authored two-project store: the session cwd belongs to /example/project.
function projects() {
  const goals = {
    '/example/project': ['t-123abc'],
    '/other/project': ['t-af18b0'],
    '/spaced key/project': ['t-bbbbbb'],
  };
  const calls = [];
  return {
    calls,
    async show(cwd, id, _signal, history = false, project) {
      calls.push({cwd, id, history, project});
      const key =
        project ?? (cwd === '/example/linked' ? '/example/project' : undefined);
      if (!goals[key]?.includes(id))
        throw new Error(
          `dots goal query failed: task ${id} does not exist in this project`,
        );
      return {...snapshot(id, `Goal from ${key}`), project: key};
    },
    async list() {
      return {project: '/example/project', goals: []};
    },
    async release() {},
  };
}

test('goals in another project follow by explicit key; later reads use the recorded project', async () => {
  const backend = projects();
  const h = harness(backend);
  await h.event('session_start');
  await assert.rejects(
    h.tool('t-af18b0'),
    error =>
      /does not exist in this project/.test(error.message) &&
      !/interrupted/.test(error.message),
  );
  assert.deepEqual(restoreGoals(h.entries), {followed: []});
  const result = await h.tool('t-af18b0', undefined, '/other/project');
  assert.deepEqual(result.details, {id: 't-af18b0', project: '/other/project'});
  await h.command('follow t-123abc');
  assert.equal(
    backend.calls.at(-1).project,
    undefined,
    'without a key, follow uses the session cwd',
  );
  assert.deepEqual(restoreGoals(h.entries).followed, [
    {id: 't-af18b0', project: '/other/project'},
    focused(),
  ]);
  backend.calls.length = 0;
  const context = (await h.context())[0].content;
  assert.match(context, /Goal from \/other\/project/);
  assert.match(context, /Goal from \/example\/project/);
  assert.deepEqual(backend.calls.map(({id, project}) => [id, project]).sort(), [
    ['t-123abc', '/example/project'],
    ['t-af18b0', '/other/project'],
  ]);
  backend.calls.length = 0;
  await h.command('show t-af18b0');
  assert.deepEqual(
    backend.calls
      .filter(call => call.id === 't-af18b0')
      .map(({id, project}) => [id, project]),
    [['t-af18b0', '/other/project']],
  );
  assert.match(h.messages.at(-1).message.content, /Project: \/other\/project/);
  // Refollowing without a key keeps the recorded project.
  assert.equal((await h.tool('t-af18b0')).details.project, '/other/project');
  const resumed = harness(backend, h.entries);
  backend.calls.length = 0;
  await resumed.event('session_start');
  assert.deepEqual(
    backend.calls.map(({id, project}) => [id, project]),
    [
      ['t-af18b0', '/other/project'],
      ['t-123abc', '/example/project'],
    ],
  );
  assert.doesNotMatch((await resumed.context())[0].content, /Unavailable/);
});

test('/goal follow names a project after the ID and reports the real failure', async () => {
  const backend = projects();
  const h = harness(backend);
  await h.event('session_start');
  await h.command('follow t-af18b0');
  assert.match(
    h.notices.at(-1)[0],
    /task t-af18b0 does not exist in this project/,
  );
  assert.equal(h.notices.at(-1)[1], 'error');
  assert.equal(
    h.notices.filter(([message]) => /does not exist/.test(message)).length,
    1,
    'one notice per failure',
  );
  await h.command('follow t-bbbbbb  /spaced key/project ');
  assert.match(h.notices.at(-1)[0], /Focused t-bbbbbb/);
  assert.deepEqual(restoreGoals(h.entries).focus, {
    id: 't-bbbbbb',
    project: '/spaced key/project',
  });
  await assert.rejects(
    h.tool('t-af18b0', undefined, '/example/project'),
    /task t-af18b0 does not exist in this project/,
  );
  assert.equal(
    restoreGoals(h.entries).focus.id,
    't-bbbbbb',
    'a failed follow keeps the previous selection',
  );
  assert.equal(restoreGoals(h.entries).followed.length, 1);
});

test('late focus reads cannot persist after shutdown or unfocus', async () => {
  for (const stop of ['shutdown', 'unfocus', 'tree']) {
    const backend = source();
    const pending = gate();
    const h = harness(backend);
    backend.show = () => pending.promise;
    await h.event('session_start');
    const focusing = h.command('focus t-123abc');
    if (stop === 'shutdown') await h.event('session_shutdown');
    else if (stop === 'tree') await h.event('session_tree');
    else await h.command('unfocus');
    pending.resolve(snapshot());
    await focusing;
    assert.equal(h.entries.filter(e => e.data.focus !== null).length, 0);
    assert.deepEqual(await h.context(), []);
  }
});

test('manual refresh shares the pre-run observation instead of releasing stale context', async () => {
  const backend = source();
  const h = harness(backend, [entry(focused())]);
  await h.event('session_start');
  const pending = gate();
  const reads = [];
  backend.show = (_cwd, id, signal) => {
    reads.push({id, signal});
    return pending.promise;
  };
  let settled = false;
  const context = h.context().then(files => {
    settled = true;
    return files;
  });
  const manual = h.command('refresh');
  await Promise.resolve();
  assert.equal(settled, false);
  assert.equal(reads.length, 1);
  assert.equal(reads[0].signal.aborted, false);
  const fresh = snapshot();
  fresh.overview.goal.briefContent = {text: 'Fresh overlapping observation'};
  pending.resolve(fresh);
  const files = await context;
  await manual;
  assert.match(files[0].content, /Fresh overlapping observation/);
  assert.doesNotMatch(files[0].content, /Use one shared export interface/);
});

test('background reads cannot cancel an explicit focus change', async () => {
  const backend = source();
  const h = harness(backend, [entry(focused())]);
  await h.event('session_start');
  const pending = gate();
  const reads = [];
  backend.show = (_cwd, id, signal) => {
    reads.push({id, signal});
    return pending.promise;
  };
  const focusing = h.command('focus t-999999');
  const background = h.event('agent_end');
  assert.equal(reads.length, 1);
  assert.equal(reads[0].id, 't-999999');
  assert.equal(reads[0].signal.aborted, false);
  pending.resolve(snapshot('t-999999', 'Explicit selection'));
  await Promise.all([focusing, background]);
  assert.equal(h.entries.at(-1).data.focus.id, 't-999999');
  assert.ok(
    h.notices.some(([message]) => message.includes('Focused t-999999')),
  );
});

test('pre-run observers follow additions and refresh existing goals after a rejected addition', async () => {
  for (const fail of [false, true]) {
    const backend = source();
    const h = harness(backend, [entry(focused())]);
    await h.event('session_start');
    const pending = gate();
    let oldSignal;
    backend.show = (_cwd, id, signal) => {
      if (id === 't-123abc' && oldSignal)
        return Promise.resolve(snapshot(id, 'Fresh original goal'));
      if (id === 't-123abc') {
        oldSignal = signal;
        return new Promise((_resolve, reject) =>
          signal.addEventListener('abort', () => reject(new Error('aborted')), {
            once: true,
          }),
        );
      }
      return pending.promise.then(() => {
        if (fail) throw new Error('Selection unavailable');
        return snapshot(id, 'Replacement goal');
      });
    };
    const context = h.context();
    const focusing = h.command('focus t-999999');
    assert.equal(oldSignal.aborted, true);
    pending.resolve();
    const files = await context;
    await focusing;
    assert.doesNotMatch(files[0].content, /Consistent exports/);
    if (fail) {
      assert.match(files[0].content, /Fresh original goal/);
      assert.doesNotMatch(
        files[0].content,
        /Selection unavailable|Replacement goal/,
      );
      assert.equal(h.entries.at(-1).data.focus.id, 't-123abc');
      assert.ok(
        !h.notices.some(([message]) => message.includes('Focused t-999999')),
      );
    } else {
      assert.match(files[0].content, /Replacement goal/);
      assert.equal(h.entries.at(-1).data.focus.id, 't-999999');
    }
  }
});

test('initial focus selection is awaited before supplying pre-run context', async () => {
  const backend = source();
  const h = harness(backend);
  await h.event('session_start');
  const pending = gate();
  backend.show = () => pending.promise;
  const focusing = h.command('focus t-123abc');
  let settled = false;
  const context = h.context().then(files => {
    settled = true;
    return files;
  });
  await Promise.resolve();
  assert.equal(settled, false);
  pending.resolve(snapshot());
  const files = await context;
  await focusing;
  assert.match(files[0].content, /Consistent exports/);
});

// A stand-in for Pi's overlay host: records options and closes on done().
function overlayHost(h, theme) {
  const host = {opened: 0, closed: 0, view: undefined};
  h.ctx.ui.custom = (factory, options) =>
    new Promise(resolve => {
      host.options = options;
      host.opened++;
      const view = factory(
        {requestRender() {}, terminal: {rows: 40, columns: 160}},
        theme,
        {
          matches: (data, action) =>
            data === '\x1b' && action === 'tui.select.cancel',
        },
        () => {
          host.closed++;
          host.view = undefined;
          view.dispose?.();
          resolve();
        },
      );
      host.view = view;
    });
  return host;
}

test('the shortcut, /goal and Esc open and close one centered popup; shutdown closes it', async () => {
  initTheme('dark', false);
  const theme = getThemeByName('dark');
  const h = harness(source(), [entry(focused())], 'tui');
  await h.event('session_start');
  const host = overlayHost(h, theme);
  const toggle = h.shortcuts.get('alt+g');
  assert.ok(toggle, 'registered shortcut');
  toggle.handler(h.ctx);
  assert.equal(host.opened, 1);
  assert.equal(host.options.overlay, true);
  assert.equal(host.options.overlayOptions.anchor, 'center');
  assert.equal(host.options.overlayOptions.nonCapturing, undefined);
  assert.match(host.view.render(120).join('\n'), /Consistent exports/);
  await h.command('');
  await h.command('show');
  assert.equal(host.opened, 1, 'one popup at a time');
  host.view.handleInput('\x1b');
  assert.equal(host.closed, 1, 'Esc closes the popup');
  await h.command('show');
  assert.equal(host.opened, 2);
  host.view.handleInput('\x1bg');
  assert.equal(host.closed, 2, 'the shortcut closes the popup');
  toggle.handler(h.ctx);
  await h.event('session_shutdown');
  assert.equal(host.closed, 3);
  assert.equal(h.userMessages.length, 0);
  assert.equal(h.messages.length, 0);
});

test('cancelled picker and failed focus preserve prior focus; malformed history can be superseded', async () => {
  const backend = source();
  const h = harness(backend, [entry(focused())]);
  await h.event('session_start');
  await h.command('focus');
  assert.equal(h.entries.length, 1);
  backend.show = async () => {
    throw new Error('not found');
  };
  await h.command('focus t-999999');
  assert.equal(h.entries.length, 1);
  assert.ok(h.notices.some(([s]) => s.includes('not found')));
  const invalid = {
    type: 'custom',
    customType: FOCUS_ENTRY,
    data: {version: 99},
  };
  assert.throws(() => restoreFocus([invalid]), /Unsupported/);
  assert.deepEqual(restoreFocus([invalid, entry(focused())]), focused());
  assert.equal(restoreFocus([invalid, entry(null)]), undefined);
});

test('multiple goals survive selection, cold resume and per-goal failures; unfollow is session-only', async () => {
  const backend = source();
  let failFirst = false;
  backend.show = async (_cwd, id) => {
    if (id === 't-123abc' && failFirst) throw new Error('First unavailable');
    const value = snapshot(
      id,
      id === 't-123abc' ? 'First goal' : 'Second goal',
    );
    value.overview.goal.brief = `/example/${id}.md`;
    value.overview.goal.briefContent = {
      text: `Accepted contract: ${value.overview.goal.brief}`,
    };
    return value;
  };
  const h = harness(backend);
  await h.event('session_start');
  await h.tool('t-123abc');
  await h.tool('t-999999');
  await h.tool('t-123abc');
  assert.equal(
    restoreGoals(h.entries).followed.length,
    2,
    'following is additive and idempotent',
  );
  assert.equal((await h.list()).details.followed.length, 2);
  const context = (await h.context())[0].content;
  assert.match(context, /First goal/);
  assert.match(context, /Second goal/);
  assert.match(context, /t-123abc.md/);
  assert.match(context, /t-999999.md/);
  await h.command('show t-999999');
  assert.equal(restoreGoals(h.entries).focus.id, 't-999999');
  assert.equal(restoreGoals(h.entries).followed.length, 2);
  const resumed = harness(backend, h.entries);
  await resumed.event('session_start');
  assert.match(
    (await resumed.context())[0].content,
    /First goal[\s\S]*Second goal/,
  );
  failFirst = true;
  const partial = (await h.context())[0].content;
  assert.match(partial, /First unavailable/);
  assert.match(partial, /Second goal/);
  assert.doesNotMatch(partial, /Accepted contract: \/example\/t-123abc.md/);
  await h.unfollow({id: 't-123abc'});
  assert.equal(restoreGoals(h.entries).followed.length, 1);
  assert.equal(
    (await resumed.list()).details.followed.length,
    2,
    'other sessions keep their own set',
  );
  assert.doesNotMatch((await h.context())[0].content, /t-123abc/);
  await h.unfollow({all: true});
  await h.unfollow({all: true});
  assert.deepEqual(await h.context(), []);
  assert.deepEqual(restoreGoals(h.entries), {followed: []});
  assert.equal(h.userMessages.length, 0);
});

test('refresh reports whole-set health, including unselected errors and brief warnings', async () => {
  const backend = source();
  const h = harness(backend);
  await h.event('session_start');
  await h.tool('t-123abc');
  await h.tool('t-999999');
  backend.show = async (_cwd, id) => {
    if (id === 't-123abc') throw new Error('First goal unavailable');
    return snapshot(id);
  };
  await h.command('refresh');
  assert.match(
    h.notices.at(-1)[0],
    /2 followed goal\(s\) refreshed.*1 need attention/,
  );
  assert.match(h.notices.at(-1)[0], /t-123abc: Error: First goal unavailable/);
  assert.equal(h.notices.at(-1)[1], 'error');
  await h.unfollow({id: 't-123abc'});
  for (const warning of [
    'Brief unavailable: removed',
    'Brief truncated; read the reference',
  ]) {
    backend.show = async (_cwd, id) => {
      const value = snapshot(id);
      value.overview.goal.briefContent = {text: '', warning};
      return value;
    };
    await h.command('refresh');
    assert.match(h.notices.at(-1)[0], /1 need attention/);
    assert.ok(h.notices.at(-1)[0].includes(warning));
  }
  backend.show = async (_cwd, id) => snapshot(id);
  await h.command('refresh');
  assert.doesNotMatch(h.notices.at(-1)[0], /need attention/);
});

test('unfollow cancels pending additions, retains other context, and validates explicit intent', async () => {
  const h = harness();
  await h.event('session_start');
  await h.tool('t-123abc');
  await h.tool('t-999999');
  await assert.rejects(h.unfollow({}), /Specify/);
  await assert.rejects(h.unfollow({id: 't-123abc', all: true}), /Specify/);
  const cancelled = new AbortController();
  cancelled.abort();
  await assert.rejects(h.unfollow({all: true}, cancelled.signal), /cancelled/);
  assert.equal((await h.list()).details.followed.length, 2);
  const pending = gate();
  let initial = true;
  h.backend.show = async (_cwd, id) => {
    if (initial) {
      initial = false;
      await pending.promise;
    }
    return snapshot(id);
  };
  const context = h.context();
  await h.unfollow({id: 't-123abc'});
  pending.resolve();
  assert.doesNotMatch((await context)[0].content, /t-123abc/);
  assert.match((await h.context())[0].content, /t-999999/);
  const addition = gate();
  h.backend.show = async (_cwd, id) => {
    await addition.promise;
    return snapshot(id);
  };
  const adding = h.tool('t-aaaaaa');
  await h.unfollow({all: true});
  addition.resolve();
  await assert.rejects(adding, /interrupted/);
  assert.deepEqual(restoreGoals(h.entries), {followed: []});
});

test('saved sets reject duplicates and inconsistent selection; restoring writes nothing', async () => {
  const first = focused();
  const second = {...focused(), id: 't-999999'};
  const saved = (focus, followed) => ({
    type: 'custom',
    customType: FOCUS_ENTRY,
    data: {version: 2, focus, followed},
  });
  assert.throws(() => restoreGoals([saved(first, [first, first])]), /Invalid/);
  assert.throws(() => restoreGoals([saved(second, [first])]), /Invalid/);
  assert.throws(() => restoreGoals([saved(null, [first])]), /Invalid/);
  const h = harness(source(), [entry(first)]);
  await h.event('session_start');
  assert.equal(h.entries.length, 1);
  assert.deepEqual(restoreGoals(h.entries), {focus: first, followed: [first]});
  await h.tool(second.id);
  h.entries.push(entry(null));
  await h.event('session_tree');
  assert.deepEqual((await h.list()).details.followed, []);
  h.entries.pop();
  await h.event('session_tree');
  assert.equal((await h.list()).details.followed.length, 2);
});

test('the summary line uses native press/click handling and cannot revive stale widgets', async () => {
  initTheme('dark', false);
  const theme = getThemeByName('dark');
  const h = harness(source(), [entry(focused())], 'tui');
  await h.event('session_start');
  const tui = {requestRender() {}, terminal: {rows: 40}};
  const widget = h.widgets.findLast(
    ([key, value]) => key === 'dots.goal' && value,
  )[1](tui, theme);
  assert.equal(widget.render(120).length, 1, 'one summary line');
  assert.match(widget.render(120).join(''), /Consistent exports/);
  const event = {
    type: 'press',
    button: 'left',
    x: 2,
    y: 1,
    screenX: 2,
    screenY: 1,
    width: 120,
    height: 20,
    shift: false,
    alt: false,
    ctrl: false,
  };
  for (const extra of [
    {type: 'wheel'},
    {type: 'drag'},
    {type: 'move'},
    {type: 'release'},
    {button: 'right'},
    {shift: true},
    {alt: true},
    {ctrl: true},
  ]) {
    assert.equal(widget.handleMouse({...event, ...extra}), undefined);
  }
  const host = overlayHost(h, theme);
  assert.equal(widget.handleMouse(event).handled, true);
  assert.equal(host.opened, 0, 'press does not open the panel');
  widget.handleMouse({...event, type: 'click'});
  assert.equal(host.opened, 1);
  widget.handleMouse({...event, type: 'click'});
  assert.equal(host.opened, 1, 'one panel at a time');
  assert.equal(
    h.entries.length,
    1,
    'inspection does not change selection or follows',
  );
  await h.event('session_tree');
  assert.equal(host.closed, 1, 'branch changes close the panel');
  assert.equal(
    widget.handleMouse({...event, type: 'click'}),
    undefined,
    'retired widgets cannot open panels',
  );
  const replacement = h.widgets.findLast(
    ([key, value]) => key === 'dots.goal' && value,
  )[1](tui, theme);
  await h.event('session_shutdown');
  assert.equal(replacement.handleMouse(event), undefined);
});

test('finished goal inspection shows closed slices without expanding ordinary agent context', async () => {
  initTheme('dark', false);
  const theme = getThemeByName('dark');
  for (const mode of ['tui', 'json']) {
    const backend = source();
    backend.show = async (_cwd, id) => {
      const full = snapshot(id);
      full.overview.goal.root.status = 'done';
      full.overview.tasks[0].status = 'done';
      Object.assign(full.overview.progress, {open: 0, ready: 0, done: 1});
      return withHierarchy(full);
    };
    const h = harness(backend, [entry(focused())], mode);
    const host = overlayHost(h, theme);
    await h.event('session_start');
    await h.command('show');
    if (mode === 'tui') {
      const output = host.view.render(100).join('\n');
      assert.match(output, /Reference slice/);
      assert.match(output, /with closed/);
    } else
      assert.doesNotMatch(h.messages.at(-1).message.content, /Reference slice/);
    const context = (await h.context())[0].content;
    assert.doesNotMatch(context, /Reference slice/);
    assert.match(context, /Tasks: 1 done\nNo unfinished tasks/);
    await h.event('session_shutdown');
  }
});

test('branch changes remove only the goal widget and headless inspection never wakes', async () => {
  const h = harness(source(), [entry(focused())], 'tui');
  await h.event('session_start');
  assert.deepEqual(h.widgets.find(([key]) => key === 'dots.goal')[2], {
    placement: 'belowEditor',
  });
  h.entries.push(entry(null));
  await h.event('session_tree');
  assert.deepEqual(
    h.widgets.findLast(([key]) => key === 'dots.goal'),
    ['dots.goal', undefined],
  );
  const headless = harness(source(), [entry(focused())], 'json');
  await headless.event('session_start');
  await headless.command('show');
  assert.equal(headless.widgets.length, 0);
  assert.equal(headless.messages[0].options.triggerTurn, false);
});

test('goal worker observation subscription is session-owned and unfocus removes animated rows', async () => {
  initTheme('dark', false);
  const theme = getThemeByName('dark');
  const h = harness(source(), [entry(focused())], 'tui');
  await h.event('session_start');
  assert.equal(h.bus.size, 1);
  const mounted = h.widgets.findLast(
    ([key, value]) => key === 'dots.goal' && value,
  )[1]({requestRender() {}}, theme);
  const render = () => mounted.render(120).join('\n');
  assert.doesNotMatch(render(), /[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]/);
  h.bus.get(WORKER_ACTIVITY_EVENT)({
    workers: [{assignment: focused(), label: 'w', state: 'running'}],
    frame: 2,
  });
  assert.match(render(), /⠹/);
  h.bus.get(WORKER_ACTIVITY_EVENT)({
    workers: [{assignment: focused(), label: 'w', state: 'quiet'}],
    frame: 3,
  });
  assert.doesNotMatch(render(), /[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]/);
  h.bus.get(WORKER_ACTIVITY_EVENT)({
    workers: [{assignment: focused(), label: 'w', state: 'running'}],
    frame: 4,
  });
  await h.unfollow({all: true});
  assert.deepEqual(mounted.render(120), []);
  await h.event('session_shutdown');
  assert.equal(h.bus.size, 0);
  await h.event('session_start');
  assert.equal(h.bus.size, 1, 'session restart installs one listener');
  await h.event('session_shutdown');
  assert.equal(h.bus.size, 0);
});

test("a worker session never discovers goal offers; that is its owner's concern", async () => {
  const previous = process.env.DOTS_WORKER_ID;
  process.env.DOTS_WORKER_ID = 'w-fixture';
  try {
    const backend = source();
    let listed = 0;
    backend.list = async () => {
      listed++;
      throw new Error('unreachable from a worker');
    };
    const h = harness(backend, [], 'tui');
    await h.event('session_start');
    await h.event('agent_end');
    assert.equal(listed, 0);
    assert.ok(!h.notices.some(notice => /unavailable/.test(String(notice[0]))));
  } finally {
    if (previous === undefined) delete process.env.DOTS_WORKER_ID;
    else process.env.DOTS_WORKER_ID = previous;
  }
});

test('ending a session releases its claims; reload keeps them and workers never release', async () => {
  for (const reason of ['quit', 'new', 'resume', 'fork']) {
    const h = harness();
    await h.event('session_start');
    await h.event('session_shutdown', {type: 'session_shutdown', reason});
    assert.deepEqual(h.backend.releases, ['pi-fixture-session'], reason);
  }
  const reload = harness();
  await reload.event('session_start');
  await reload.event('session_shutdown', {
    type: 'session_shutdown',
    reason: 'reload',
  });
  assert.deepEqual(reload.backend.releases, []);
  process.env.DOTS_WORKER_ID = 'w-fixture';
  const worker = harness();
  await worker.event('session_start');
  await worker.event('session_shutdown', {
    type: 'session_shutdown',
    reason: 'quit',
  });
  assert.deepEqual(worker.backend.releases, []);
  delete process.env.DOTS_WORKER_ID;
  const failing = harness(source(), [], 'tui');
  failing.backend.releaseFailure = 'catalog unavailable';
  await failing.event('session_start');
  await failing.event('session_shutdown', {
    type: 'session_shutdown',
    reason: 'quit',
  });
  assert.deepEqual(failing.backend.releases, ['pi-fixture-session']);
  assert.match(
    failing.notices.map(([text]) => text).join('\n'),
    /Claims not released: Error: catalog unavailable/,
  );
});
