import assert from "node:assert/strict";
import test from "node:test";
import goal, { FOCUS_ENTRY, restoreFocus } from "../extensions/goal/index.ts";
import { source, snapshot } from "./goal-fixture.mjs";

const entry = (focus) => ({ type: "custom", customType: FOCUS_ENTRY, data: { version: 1, focus } });
const focused = () => ({ id: "t-123abc", project: "/example/project" });
function harness(backend = source(), saved = [], mode = "rpc") {
  const handlers = new Map(), commands = new Map(), tools = new Map(), entries = [...saved], notices = [], messages = [], userMessages = [], widgets = [];
  const pi = {
    on(name, handler) { handlers.set(name, handler); },
    registerCommand(name, command) { commands.set(name, command); },
    registerTool(tool) { tools.set(tool.name, tool); },
    sendUserMessage(message, options) { userMessages.push({ message, options }); },
    appendEntry(customType, data) { entries.push({ type: "custom", customType, data }); },
    sendMessage(message, options) { messages.push({ message, options }); },
  };
  goal(pi, backend);
  const ctx = { cwd: "/example/linked", mode, hasUI: mode !== "json", sessionManager: { getBranch: () => entries },
    ui: { notify: (...args) => notices.push(args), setWidget: (...args) => widgets.push(args), select: async () => undefined },
  };
  return { backend, entries, notices, messages, userMessages, widgets, ctx,
    tool: (id, signal) => tools.get("goal_focus").execute("fixture", { id }, signal, undefined, ctx),
    event: (name, data = {}) => handlers.get(name)?.(data, ctx),
    command: (args) => commands.get("goal").handler(args, ctx),
    async context() { const event = { systemPromptOptions: { contextFiles: [] } }; await handlers.get("before_agent_start")(event, ctx); return event.systemPromptOptions.contextFiles; },
  };
}
function gate() { let resolve; const promise = new Promise((r) => { resolve = r; }); return { promise, resolve }; }

test("new starts a planning conversation without writing tasks or changing focus", async () => {
  const h = harness();
  await h.event("session_start");
  await h.command("new Consistent exports across all formats");
  assert.equal(h.userMessages.length, 1);
  assert.match(h.userMessages[0].message, /Consistent exports across all formats/);
  assert.match(h.userMessages[0].message, /dots task goal create/);
  assert.match(h.userMessages[0].message, /goal_focus/);
  assert.match(h.userMessages[0].message, /not to start implementation/);
  assert.equal(h.backend.calls.length, 0);
  assert.equal(h.entries.length, 0);
});

test("agent can focus after creation; cancelled tool reads cannot persist focus", async () => {
  const h = harness();
  await h.event("session_start");
  const result = await h.tool("t-123abc");
  assert.equal(result.details.id, "t-123abc");
  assert.match(result.content[0].text, /Use one shared export interface/);
  const pending = gate(), signal = new AbortController();
  h.backend.show = () => pending.promise;
  const change = h.tool("t-999999", signal.signal);
  signal.abort();
  pending.resolve(snapshot("t-999999"));
  await assert.rejects(change, /interrupted/);
  assert.equal(h.entries.at(-1).data.focus.id, "t-123abc");
  assert.equal(h.userMessages.length, 0);
});

test("history is explicit and never replaces active-only agent orientation", async () => {
  const backend = source(), h = harness(backend, [entry(focused())]);
  const histories = [];
  backend.show = async (_cwd, id, _signal, history = false) => {
    histories.push(history);
    const value = snapshot(id);
    value.overview.progress.done = 1;
    value.overview.progress.total = 2;
    value.overview.hidden_closed = history ? 0 : 1;
    if (history) value.overview.tasks.unshift({ ...value.overview.tasks[0], id: "t-111111", title: "Old finished slice", status: "done" });
    return value;
  };
  await h.event("session_start");
  await h.command("history");
  assert.match(h.messages.at(-1).message.content, /Old finished slice/);
  assert.equal(h.messages.at(-1).options.triggerTurn, false);
  const files = await h.context();
  assert.match(files[0].content, /1 closed task details hidden/);
  assert.doesNotMatch(files[0].content, /Old finished slice/);
  assert.equal(histories.at(-1), false);
});

test("unfocused sessions do not query dots; focus persists identity without execution", async () => {
  const h = harness();
  await h.event("session_start");
  assert.deepEqual(await h.context(), []);
  assert.equal(h.backend.calls.length, 0);
  await h.command("focus t-123abc");
  assert.deepEqual(h.entries, [entry(focused())]);
  const files = await h.context();
  assert.equal(files.length, 1);
  assert.match(files[0].content, /Use one shared export interface/);
  assert.match(files[0].content, /does not authorize/);
  assert.match(files[0].content, /Reference slice/);
  await h.command("show");
  assert.equal(h.messages.length, 1);
  assert.equal(h.messages[0].options.triggerTurn, false);
  await h.command("unfocus");
  assert.equal(h.entries.at(-1).data.focus, null);
  assert.deepEqual(await h.context(), []);
  assert.ok(h.messages.every((m) => m.options.triggerTurn === false));
});

test("cold resume and branch navigation restore focus but never resume work", async () => {
  const h = harness(source(), [entry(focused())]);
  await h.event("session_start");
  assert.equal(h.backend.calls.length, 1);
  assert.match((await h.context())[0].content, /Consistent exports/);
  h.entries.push(entry(null));
  await h.event("session_tree");
  assert.deepEqual(await h.context(), []);
  h.entries.pop();
  await h.event("session_tree");
  assert.match((await h.context())[0].content, /Consistent exports/);
  assert.equal(h.messages.length, 0);
  assert.equal(h.entries.length, 1);
});

test("refresh uses changed brief; unavailable data replaces old context, not a false success", async () => {
  const backend = source(), h = harness(backend, [entry(focused())]);
  await h.event("session_start");
  backend.brief = async () => ({ text: "Changed accepted pattern" });
  assert.match((await h.context())[0].content, /Changed accepted pattern/);
  backend.brief = async () => ({ text: "", warning: "Brief unavailable: removed" });
  assert.match((await h.context())[0].content, /Brief unavailable/);
  backend.show = async () => { throw new Error("dots unsupported"); };
  const content = (await h.context())[0].content;
  assert.match(content, /Unavailable: Error: dots unsupported/);
  assert.doesNotMatch(content, /Changed accepted pattern|Use one shared export interface|1 ready/);
});

test("saved project mismatch is explicit and does not load another project's brief", async () => {
  const backend = source(), h = harness(backend, [entry({ ...focused(), project: "/other" })]);
  backend.brief = async () => { throw new Error("must not read"); };
  await h.event("session_start");
  assert.match((await h.context())[0].content, /different project/);
});

test("late focus reads cannot persist after shutdown or unfocus", async () => {
  for (const stop of ["shutdown", "unfocus", "tree"]) {
    const backend = source(), pending = gate(), h = harness(backend);
    backend.show = () => pending.promise;
    await h.event("session_start");
    const focusing = h.command("focus t-123abc");
    if (stop === "shutdown") await h.event("session_shutdown");
    else if (stop === "tree") await h.event("session_tree");
    else await h.command("unfocus");
    pending.resolve(snapshot());
    await focusing;
    assert.equal(h.entries.filter((e) => e.data.focus !== null).length, 0);
    assert.deepEqual(await h.context(), []);
  }
});

test("manual refresh shares the pre-run observation instead of releasing stale context", async () => {
  const backend = source(), h = harness(backend, [entry(focused())]);
  await h.event("session_start");
  const pending = gate(), reads = [];
  backend.show = (_cwd, id, signal) => { reads.push({ id, signal }); return pending.promise; };
  backend.brief = async () => ({ text: "Fresh overlapping observation" });
  let settled = false;
  const context = h.context().then((files) => { settled = true; return files; });
  const manual = h.command("refresh");
  await Promise.resolve();
  assert.equal(settled, false);
  assert.equal(reads.length, 1);
  assert.equal(reads[0].signal.aborted, false);
  pending.resolve(snapshot());
  const files = await context;
  await manual;
  assert.match(files[0].content, /Fresh overlapping observation/);
  assert.doesNotMatch(files[0].content, /Use one shared export interface/);
});

test("background reads cannot cancel an explicit focus change", async () => {
  const backend = source(), h = harness(backend, [entry(focused())]);
  await h.event("session_start");
  const pending = gate(), reads = [];
  backend.show = (_cwd, id, signal) => { reads.push({ id, signal }); return pending.promise; };
  const focusing = h.command("focus t-999999");
  const background = h.event("agent_end");
  assert.equal(reads.length, 1);
  assert.equal(reads[0].id, "t-999999");
  assert.equal(reads[0].signal.aborted, false);
  pending.resolve(snapshot("t-999999", "Explicit selection"));
  await Promise.all([focusing, background]);
  assert.equal(h.entries.at(-1).data.focus.id, "t-999999");
  assert.ok(h.notices.some(([message]) => message.includes("Focused t-999999")));
});

test("pre-run observers follow superseding focus and receive failures instead of stale context", async () => {
  for (const fail of [false, true]) {
    const backend = source(), h = harness(backend, [entry(focused())]);
    await h.event("session_start");
    const pending = gate();
    let oldSignal;
    backend.show = (_cwd, id, signal) => {
      if (id === "t-123abc") {
        oldSignal = signal;
        return new Promise((_resolve, reject) => signal.addEventListener("abort", () => reject(new Error("aborted")), { once: true }));
      }
      return pending.promise.then(() => { if (fail) throw new Error("Selection unavailable"); return snapshot(id, "Replacement goal"); });
    };
    const context = h.context();
    const focusing = h.command("focus t-999999");
    assert.equal(oldSignal.aborted, true);
    pending.resolve();
    const files = await context;
    await focusing;
    assert.doesNotMatch(files[0].content, /Consistent exports/);
    if (fail) {
      assert.match(files[0].content, /Selection unavailable/);
      assert.doesNotMatch(files[0].content, /Use one shared export interface/);
      assert.equal(h.entries.at(-1).data.focus.id, "t-123abc");
      assert.ok(!h.notices.some(([message]) => message.includes("Focused t-999999")));
    } else {
      assert.match(files[0].content, /Replacement goal/);
      assert.equal(h.entries.at(-1).data.focus.id, "t-999999");
    }
  }
});

test("initial focus selection is awaited before supplying pre-run context", async () => {
  const backend = source(), h = harness(backend);
  await h.event("session_start");
  const pending = gate();
  backend.show = () => pending.promise;
  const focusing = h.command("focus t-123abc");
  let settled = false;
  const context = h.context().then((files) => { settled = true; return files; });
  await Promise.resolve();
  assert.equal(settled, false);
  pending.resolve(snapshot());
  const files = await context;
  await focusing;
  assert.match(files[0].content, /Consistent exports/);
});

test("overlapping inspections reserve one screen and shutdown closes its command", async () => {
  const backend = source(), h = harness(backend, [entry(focused())], "tui");
  await h.event("session_start");
  const pending = gate(), opened = gate();
  backend.show = () => pending.promise;
  let screens = 0;
  h.ctx.ui.custom = (factory) => new Promise((resolve) => {
    screens++;
    const view = factory({ requestRender() {}, terminal: { rows: 40 } }, {}, { matches: () => false }, () => { view.dispose(); resolve(); });
    opened.resolve();
  });
  const first = h.command("show");
  await h.command("show");
  assert.ok(h.notices.some(([message]) => message.includes("already open")));
  pending.resolve(snapshot());
  await opened.promise;
  assert.equal(screens, 1);
  await h.event("session_shutdown");
  await first;
});

test("cancelled picker and failed focus preserve prior focus; malformed history can be superseded", async () => {
  const backend = source(), h = harness(backend, [entry(focused())]);
  await h.event("session_start");
  await h.command("focus");
  assert.equal(h.entries.length, 1);
  backend.show = async () => { throw new Error("not found"); };
  await h.command("focus t-999999");
  assert.equal(h.entries.length, 1);
  assert.ok(h.notices.some(([s]) => s.includes("not found")));
  const invalid = { type: "custom", customType: FOCUS_ENTRY, data: { version: 99 } };
  assert.throws(() => restoreFocus([invalid]), /Unsupported/);
  assert.deepEqual(restoreFocus([invalid, entry(focused())]), focused());
  assert.equal(restoreFocus([invalid, entry(null)]), undefined);
});

test("branch changes remove only the goal widget and headless inspection never wakes", async () => {
  const h = harness(source(), [entry(focused())], "tui");
  await h.event("session_start");
  assert.equal(h.widgets[0][0], "dots.goal");
  h.entries.push(entry(null));
  await h.event("session_tree");
  assert.deepEqual(h.widgets.at(-1), ["dots.goal", undefined]);
  const headless = harness(source(), [entry(focused())], "json");
  await headless.event("session_start");
  await headless.command("show");
  assert.equal(headless.widgets.length, 0);
  assert.equal(headless.messages[0].options.triggerTurn, false);
});
