import assert from "node:assert/strict";
import test from "node:test";
import { initTheme } from "@earendil-works/pi-coding-agent";
import { getThemeByName } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import goal, { FOCUS_ENTRY, restoreFocus, restoreGoals } from "../extensions/goal/index.ts";
import { source, snapshot } from "./goal-fixture.mjs";
import { withHierarchy } from "../extensions/goal/source.ts";

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
    unfollow: (args, signal) => tools.get("goal_unfocus").execute("fixture", args, signal, undefined, ctx),
    list: () => tools.get("goal_list").execute("fixture", {}, undefined, undefined, ctx),
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
  assert.equal(h.entries.length, 1);
  assert.deepEqual(restoreGoals(h.entries), { focus: focused(), followed: [focused()] });
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

test("pre-run observers follow additions and refresh existing goals after a rejected addition", async () => {
  for (const fail of [false, true]) {
    const backend = source(), h = harness(backend, [entry(focused())]);
    await h.event("session_start");
    const pending = gate();
    let oldSignal;
    backend.show = (_cwd, id, signal) => {
      if (id === "t-123abc" && oldSignal) return Promise.resolve(snapshot(id, "Fresh original goal"));
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
      assert.match(files[0].content, /Fresh original goal/);
      assert.doesNotMatch(files[0].content, /Selection unavailable|Replacement goal/);
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

test("multiple goals survive selection, cold resume and per-goal failures; unfollow is session-only", async () => {
  const backend = source();
  let failFirst = false;
  backend.show = async (_cwd, id) => {
    if (id === "t-123abc" && failFirst) throw new Error("First unavailable");
    const value = snapshot(id, id === "t-123abc" ? "First goal" : "Second goal");
    value.overview.goal.brief = `/example/${id}.md`;
    return value;
  };
  backend.brief = async (path) => ({ text: `Accepted contract: ${path}` });
  const h = harness(backend);
  await h.event("session_start");
  await h.tool("t-123abc"); await h.tool("t-999999"); await h.tool("t-123abc");
  assert.equal(restoreGoals(h.entries).followed.length, 2, "following is additive and idempotent");
  assert.equal((await h.list()).details.followed.length, 2);
  const context = (await h.context())[0].content;
  assert.match(context, /First goal/); assert.match(context, /Second goal/);
  assert.match(context, /t-123abc.md/); assert.match(context, /t-999999.md/);
  await h.command("show t-999999");
  assert.equal(restoreGoals(h.entries).focus.id, "t-999999");
  assert.equal(restoreGoals(h.entries).followed.length, 2);
  const resumed = harness(backend, h.entries);
  await resumed.event("session_start");
  assert.match((await resumed.context())[0].content, /First goal[\s\S]*Second goal/);
  failFirst = true;
  const partial = (await h.context())[0].content;
  assert.match(partial, /First unavailable/); assert.match(partial, /Second goal/);
  assert.doesNotMatch(partial, /Accepted contract: \/example\/t-123abc.md/);
  await h.unfollow({ id: "t-123abc" });
  assert.equal(restoreGoals(h.entries).followed.length, 1);
  assert.equal((await resumed.list()).details.followed.length, 2, "other sessions keep their own set");
  assert.doesNotMatch((await h.context())[0].content, /t-123abc/);
  await h.unfollow({ all: true }); await h.unfollow({ all: true });
  assert.deepEqual(await h.context(), []);
  assert.deepEqual(restoreGoals(h.entries), { followed: [] });
  assert.equal(h.userMessages.length, 0);
});

test("refresh reports whole-set health, including unselected errors and brief warnings", async () => {
  const backend = source(), h = harness(backend);
  await h.event("session_start");
  await h.tool("t-123abc"); await h.tool("t-999999");
  backend.show = async (_cwd, id) => {
    if (id === "t-123abc") throw new Error("First goal unavailable");
    return snapshot(id);
  };
  await h.command("refresh");
  assert.match(h.notices.at(-1)[0], /2 followed goal\(s\) refreshed.*1 need attention/);
  assert.match(h.notices.at(-1)[0], /t-123abc: Error: First goal unavailable/);
  assert.equal(h.notices.at(-1)[1], "error");
  await h.unfollow({ id: "t-123abc" });
  for (const warning of ["Brief unavailable: removed", "Brief truncated; read the reference"]) {
    backend.brief = async () => ({ text: "", warning });
    await h.command("refresh");
    assert.match(h.notices.at(-1)[0], /1 need attention/);
    assert.ok(h.notices.at(-1)[0].includes(warning));
  }
  backend.brief = async () => ({ text: "Healthy brief" });
  await h.command("refresh");
  assert.doesNotMatch(h.notices.at(-1)[0], /need attention/);
});

test("unfollow cancels pending additions, retains other context, and validates explicit intent", async () => {
  const h = harness(); await h.event("session_start");
  await h.tool("t-123abc"); await h.tool("t-999999");
  await assert.rejects(h.unfollow({}), /Specify/);
  await assert.rejects(h.unfollow({ id: "t-123abc", all: true }), /Specify/);
  const cancelled = new AbortController(); cancelled.abort();
  await assert.rejects(h.unfollow({ all: true }, cancelled.signal), /cancelled/);
  assert.equal((await h.list()).details.followed.length, 2);
  const pending = gate(); let initial = true;
  h.backend.show = async (_cwd, id) => { if (initial) { initial = false; await pending.promise; } return snapshot(id); };
  const context = h.context();
  await h.unfollow({ id: "t-123abc" }); pending.resolve();
  assert.doesNotMatch((await context)[0].content, /t-123abc/);
  assert.match((await h.context())[0].content, /t-999999/);
  const addition = gate(); h.backend.show = async (_cwd, id) => { await addition.promise; return snapshot(id); };
  const adding = h.tool("t-aaaaaa");
  await h.unfollow({ all: true }); addition.resolve();
  await assert.rejects(adding, /interrupted/);
  assert.deepEqual(restoreGoals(h.entries), { followed: [] });
});

test("saved sets reject duplicates and inconsistent selection; legacy state migrates without writes", async () => {
  const first = focused(), second = { ...focused(), id: "t-999999" };
  const saved = (focus, followed) => ({ type: "custom", customType: FOCUS_ENTRY, data: { version: 2, focus, followed } });
  assert.throws(() => restoreGoals([saved(first, [first, first])]), /Invalid/);
  assert.throws(() => restoreGoals([saved(second, [first])]), /Invalid/);
  assert.throws(() => restoreGoals([saved(null, [first])]), /Invalid/);
  const h = harness(source(), [entry(first)]); await h.event("session_start");
  assert.equal(h.entries.length, 1);
  assert.deepEqual(restoreGoals(h.entries), { focus: first, followed: [first] });
  await h.tool(second.id);
  h.entries.push(entry(null)); await h.event("session_tree");
  assert.deepEqual((await h.list()).details.followed, []);
  h.entries.pop(); await h.event("session_tree");
  assert.equal((await h.list()).details.followed.length, 2);
});

test("dashboard uses native press/click handling and cannot revive stale widgets", async () => {
  initTheme("dark", false);
  const theme = getThemeByName("dark");
  const h = harness(source(), [entry(focused())], "tui");
  await h.event("session_start");
  const tui = { requestRender() {}, terminal: { rows: 40 } };
  const widget = h.widgets.at(-1)[1](tui, theme);
  const event = { type: "press", button: "left", x: 2, y: 1, screenX: 2, screenY: 1, width: 120, height: 20, shift: false, alt: false, ctrl: false };
  for (const extra of [{ type: "wheel" }, { type: "drag" }, { type: "move" }, { type: "release" }, { button: "right" }, { shift: true }, { alt: true }, { ctrl: true }]) {
    assert.equal(widget.handleMouse({ ...event, ...extra }), undefined);
  }
  const before = h.backend.calls.length;
  assert.equal(widget.handleMouse(event).handled, true);
  assert.equal(h.backend.calls.length, before, "press does not open inspection");
  let inspector;
  const opened = gate(), closed = gate();
  h.ctx.ui.custom = async (factory, options) => {
    assert.equal(options.overlay, true);
    assert.equal(options.overlayOptions.anchor, "center");
    assert.equal(options.overlayOptions.maxHeight, "80%");
    inspector = factory(tui, theme, { matches: (data, action) => data === "\r" && action === "tui.select.confirm" }, () => {
      inspector.dispose(); closed.resolve();
    });
    opened.resolve();
    await closed.promise;
  };
  widget.handleMouse({ ...event, type: "click" });
  assert.equal(widget.handleMouse({ ...event, type: "click" }), undefined, "one dialog at a time");
  await opened.promise;
  assert.match(inspector.render(120).join("\n"), /Consistent exports/);
  inspector.handleInput("\r");
  await closed.promise;
  assert.equal(h.userMessages.length, 0);
  assert.equal(h.messages.length, 0);
  assert.equal(h.entries.length, 1, "inspection does not change selection or follows");
  await h.event("session_tree");
  assert.equal(widget.handleMouse({ ...event, type: "click" }), undefined, "retired widgets cannot open dialogs");
  const replacement = h.widgets.at(-1)[1](tui, theme);
  await h.event("session_shutdown");
  assert.equal(replacement.handleMouse(event), undefined);
});

test("finished goal inspection shows closed slices without expanding ordinary agent context", async () => {
  initTheme("dark", false);
  const theme = getThemeByName("dark");
  for (const mode of ["tui", "json"]) {
    const backend = source();
    backend.show = async (_cwd, id, _signal, history = false) => {
      const full = snapshot(id);
      full.overview.goal.root.status = "done";
      full.overview.tasks[0].status = "done";
      Object.assign(full.overview.progress, { open: 0, ready: 0, done: 1 });
      return withHierarchy(full, history);
    };
    const h = harness(backend, [entry(focused())], mode);
    let output;
    h.ctx.ui.custom = async (factory, options) => {
      assert.equal(options.overlay, true);
      const view = factory({ requestRender() {}, terminal: { rows: 40 } }, theme, { matches: () => false }, () => {});
      output = view.render(100).join("\n");
      view.dispose();
    };
    await h.event("session_start");
    await h.command("show");
    if (mode === "tui") { assert.match(output, /Reference slice/); assert.match(output, /history/); }
    else assert.doesNotMatch(h.messages.at(-1).message.content, /Reference slice/);
    const context = (await h.context())[0].content;
    assert.doesNotMatch(context, /Reference slice/);
    assert.match(context, /1 closed task details hidden/);
  }
});

test("branch changes remove only the goal widget and headless inspection never wakes", async () => {
  const h = harness(source(), [entry(focused())], "tui");
  await h.event("session_start");
  assert.equal(h.widgets[0][0], "dots.goal");
  assert.deepEqual(h.widgets[0][2], { placement: "belowEditor" });
  h.entries.push(entry(null));
  await h.event("session_tree");
  assert.deepEqual(h.widgets.at(-1), ["dots.goal", undefined]);
  const headless = harness(source(), [entry(focused())], "json");
  await headless.event("session_start");
  await headless.command("show");
  assert.equal(headless.widgets.length, 0);
  assert.equal(headless.messages[0].options.triggerTurn, false);
});
