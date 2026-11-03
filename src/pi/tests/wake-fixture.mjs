// Shared wake test fixtures: a manual clock and a session harness around the extension.
import register from '../extensions/wake/index.ts';

// A fake clock: timers fire only when the test advances time, in order.
export function fakeClock(start = Date.UTC(2026, 0, 1)) {
  let now = start;
  let seq = 0;
  const timers = new Map();
  return {
    now: () => now,
    setTimeout: (fn, ms) => {
      const id = ++seq;
      timers.set(id, {at: now + ms, fn});
      return id;
    },
    clearTimeout: id => timers.delete(id),
    pending: () => timers.size,
    async advance(ms) {
      const target = now + ms;
      for (;;) {
        const due = [...timers.entries()]
          .filter(([, t]) => t.at <= target)
          .sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        now = due[1].at;
        timers.delete(due[0]);
        due[1].fn();
        await new Promise(resolve => setImmediate(resolve)); // let async fire() settle
        await new Promise(resolve => setImmediate(resolve));
      }
      now = target;
    },
  };
}

// A session file: entries in file order with ids; `branch` is the current leaf's path (all of it unless a test moves it).
// Runs finish at once unless a test calls `busy()`; `turn()` then settles that run.
export function harness(
  clock,
  run,
  {session = 'session-a', mode = 'rpc', rex, env = {}} = {},
) {
  const listeners = {};
  const handlers = {};
  let idle = true;
  const tools = new Map();
  const messages = [];
  const widgets = new Map();
  const entries = [];
  let branch;
  let next = 0;
  const pi = {
    on: (name, fn) => {
      (listeners[name] ??= []).push(fn);
      handlers[name] = (event, context) => {
        let result;
        for (const listener of listeners[name])
          result = listener(event, context);
        return result;
      };
    },
    registerTool: tool => tools.set(tool.name, tool),
    registerCommand: () => {},
    registerMessageRenderer: () => {},
    sendMessage: (message, options) => messages.push({message, options}),
    appendEntry: (customType, data) => {
      const entry = {type: 'custom', id: `e${++next}`, customType, data};
      entries.push(entry);
      branch?.push(entry);
    },
    exec: async () => {
      throw new Error('exec should be stubbed');
    },
  };
  register(pi, {clock, run, env, ...(rex ? {rex} : {})});
  const notices = [];
  const ctx = {
    cwd: '/project',
    hasUI: true,
    mode,
    isIdle: () => idle,
    ui: {
      notify: (message, level) => notices.push({message, level}),
      setWidget: (key, factory) =>
        factory ? widgets.set(key, factory) : widgets.delete(key),
    },
    sessionManager: {
      getSessionId: () => session,
      getEntries: () => entries,
      getBranch: () => branch ?? entries,
    },
  };
  /** Moves the leaf, as /tree does: the new branch shares none of the entries so far. */
  const navigate = () => {
    branch = [];
    handlers.session_tree?.({type: 'session_tree'}, ctx);
  };
  const call = async (name, params) =>
    (await tools.get(name).execute('c', params, undefined, undefined, ctx))
      .details;
  const text = async (name, params) =>
    (await tools.get(name).execute('c', params, undefined, undefined, ctx))
      .content[0].text;
  /** A run is in progress until `turn()`. */
  const busy = () => {
    idle = false;
  };
  /** The run settles; held messages are delivered on the next macrotask. */
  const turn = async ({aborted = false} = {}) => {
    idle = true;
    handlers.agent_settled?.({type: 'agent_settled', aborted}, ctx);
    await new Promise(resolve => setTimeout(resolve, 5));
  };
  /** The user prompts, which ends a cancellation. */
  const prompt = () =>
    handlers.before_agent_start?.({messageType: 'user', prompt: 'go on'}, ctx);
  return {
    handlers,
    tools,
    messages,
    widgets,
    entries,
    notices,
    ctx,
    call,
    text,
    busy,
    turn,
    prompt,
    navigate,
  };
}
