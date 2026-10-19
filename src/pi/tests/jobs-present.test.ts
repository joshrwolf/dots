import assert from "node:assert/strict";
import test from "node:test";
import { initTheme, type ExtensionContext, type ExtensionUIContext, type KeybindingsManager, type Theme } from "@earendil-works/pi-coding-agent";
import { stripTerminalSequences, visibleWidth, type Component, type TUI } from "@earendil-works/pi-tui";
// Version-pinned theme fixture only; production consumes the public supplied Theme.
import { getThemeByName } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { emptyFacts, type Job } from "../extensions/jobs/protocol.ts";
import { JobsWidget, canStop, inspectCard, projectJob } from "../extensions/jobs/present.ts";
import { JOBS_WIDGET, JobsPresentation, type JobSource } from "../extensions/jobs/ui.ts";

initTheme("dark", false);
const loadedTheme = getThemeByName("dark");
assert.ok(loadedTheme);
const theme: Theme = loadedTheme;
const plain = (lines: string[]) => stripTerminalSequences(lines.join("\n"));
function fixture(): Job {
  return { version: 1, id: "j-00000000-0000-0000-0000-000000000001", command: "printf '日本語 👩🏽‍💻 fixture'", cwd: "/tmp/fixture", directory: "/tmp/fixture/job", timeoutMs: 10000, startedAt: 1, finishedAt: null, state: "running", stopIntent: null, facts: emptyFacts(), supervisorExit: null, observationLoss: null, artifactsEvicted: false };
}

test("projection separates root outcome, cleanup and unknown; snapshots are immutable", () => {
  const job = fixture();
  job.state = "stopping";
  job.facts.root = { code: 0, signal: null, error: null };
  const model = projectJob(job);
  assert.equal(model.root, "root exit 0");
  assert.equal(model.cleanup, "cleanup in progress");
  assert.equal(model.stoppable, false);
  assert.ok(Object.isFrozen(model));
  assert.ok(Object.isFrozen(model.notices));
  job.facts.root.code = 7;
  job.command = "changed";
  assert.equal(model.root, "root exit 0");
  assert.doesNotMatch(model.subject, /changed/);
  job.state = "unknown";
  job.observationLoss = "Supervisor lost";
  const unknown = projectJob(job);
  assert.equal(unknown.cleanup, "cleanup unknown");
  assert.equal(unknown.root, "root exit 7");
  assert.equal(unknown.stoppable, false);
  assert.ok(unknown.notices.some((n) => /Supervisor lost/.test(n.text)));
  assert.equal(canStop(job), false);
});

test("shared widget and inspection fit narrow/wide panes, sanitize controls, and retain paths/outcomes", () => {
  const job = fixture();
  job.command = "echo \x1b]52;c;payload\x07日本語\t👩🏽‍💻\nfixture";
  job.directory = "/tmp/\x1b[31mfixture\x1b[0m/job";
  job.state = "unknown";
  job.observationLoss = "cleanup \x1b[2Junknown";
  job.facts.root = { code: 9, signal: null, error: null };
  job.facts.capture.capped = true;
  const model = projectJob(job);
  const widget = new JobsWidget();
  widget.update([model], theme);
  const card = inspectCard(model, theme);
  for (const width of [1, 2, 3, 8, 16, 30, 60, 100, 180]) {
    for (const component of [widget, card]) {
      const lines = component.render(width);
      assert.ok(lines.every((line) => visibleWidth(line) <= width), `width ${width}`);
      const text = plain(lines);
      assert.doesNotMatch(text, /\x1b|\x07|\uFFFD|payload/);
    }
  }
  const wide = plain(card.render(180));
  assert.match(wide, /root exit 9/);
  assert.match(wide, /cleanup unknown/);
  assert.match(wide, /Output prefix: \/tmp\/fixture\/job\/output\.log/);
  assert.match(wide, /Recent tail: \/tmp\/fixture\/job\/tail\.log/);
  assert.match(wide, /Capture is not confirmed complete/);
  assert.doesNotMatch(wide, /failed/); // Root diagnostic, no redundant failed badge.
  assert.match(plain(widget.render(180)), /job +echo.*unknown.*root exit 9/);
  assert.match(plain(widget.render(30)), /unknown/);
  assert.equal(job.directory.includes("\x1b"), true); // Raw records unchanged.
});

test("widget bounds retained rows and distinguishes unknown history from active work", () => {
  const jobs = Array.from({ length: 32 }, (_, n) => {
    const j = fixture(); j.id = `j-${String(n).padStart(36, "0")}`; j.state = "unknown"; return projectJob(j);
  });
  const widget = new JobsWidget();
  widget.update(jobs, theme);
  const lines = widget.render(100);
  assert.equal(lines.length, 6);
  assert.match(plain(lines), /32 unknown/);
  assert.match(plain(lines), /\+28 more/);
});

type Widget = Component & { dispose?(): void };
type WidgetFactory = (tui: TUI, theme: Theme) => Widget;
function harness(mode: ExtensionContext["mode"] = "tui", hasUI = true) {
  let job = fixture();
  let reads = 0;
  let stops = 0;
  let redraws = 0;
  let widget: Widget | undefined;
  let widgetFactory: WidgetFactory | undefined;
  let inspector: Widget | undefined;
  let finishInspect: (() => void) | undefined;
  const choices: string[][] = [];
  const notifications: string[] = [];
  const widgetChanges: (string | undefined)[] = [];
  let choose: (options: string[]) => Promise<string | undefined> = async () => undefined;
  let confirm: () => Promise<boolean> = async () => false;
  let deferWidget = false;
  let brokenWidget = false;
  // Narrow host fakes: these factories only need redraw and selection bindings.
  const tui = { terminal: { rows: 40 }, requestRender: () => { redraws++; } } as TUI;
  const bindings = { matches: (data: string, action: string) => (data === "\u001b" && action === "tui.select.cancel") || (data === "\r" && action === "tui.select.confirm") } as KeybindingsManager;
  const custom: ExtensionUIContext["custom"] = (factory) => new Promise((resolve, reject) => {
    const done = (value: unknown) => { inspector?.dispose?.(); resolve(value as never); };
    finishInspect = () => done(undefined);
    Promise.resolve(factory(tui, theme, bindings, done)).then((view) => { inspector = view; }, reject);
  });
  const ui: ConstructorParameters<typeof JobsPresentation>[0]["ui"] = {
    select: async (_title, options) => { choices.push([...options]); return choose(options); },
    confirm: async () => confirm(),
    notify: (text) => { notifications.push(text); },
    custom,
    setWidget(_key, content) {
      if (brokenWidget) throw new Error("fixture UI failure");
      assert.equal(_key, JOBS_WIDGET);
      widget?.dispose?.();
      widget = undefined;
      widgetFactory = typeof content === "function" ? content : undefined;
      widgetChanges.push(widgetFactory ? _key : undefined);
      if (widgetFactory && !deferWidget) widget = widgetFactory(tui, theme);
    },
  };
  const source: JobSource = {
    list: () => { reads++; return [structuredClone(job)]; },
    get: (id) => { reads++; assert.equal(id, job.id); return structuredClone(job); },
    hasLive: () => job.state === "running" || job.state === "stopping",
    stop: (id) => { assert.equal(id, job.id); stops++; job.stopIntent = "stop"; job.state = "stopping"; return structuredClone(job); },
  };
  const presentation = new JobsPresentation({ mode, hasUI, ui }, source);
  return {
    presentation, job: () => job, replace: (next: Job) => { job = next; },
    reads: () => reads, stops: () => stops, redraws: () => redraws,
    widget: () => widget, inspector: () => inspector, factory: () => widgetFactory,
    choices, notifications, widgetChanges,
    choose: (fn: typeof choose) => { choose = fn; }, confirm: (fn: typeof confirm) => { confirm = fn; },
    finishInspect: () => finishInspect?.(), deferWidget: () => { deferWidget = true; },
    breakWidget: () => { brokenWidget = true; },
  };
}

test("picker cancellation and inspection have no stop side effects; inspection refreshes outside render", async () => {
  const h = harness();
  try {
    await h.presentation.open();
    assert.equal(h.stops(), 0);
    assert.equal(h.inspector(), undefined);
    h.choose(async (options) => options[0]);
    const opened = h.presentation.open();
    await new Promise((r) => setImmediate(r));
    assert.ok(h.inspector());
    const reads = h.reads();
    assert.match(plain(h.inspector()!.render(100)), /\/tmp\/fixture\/job\/output.log/);
    h.inspector()!.render(30);
    assert.equal(h.reads(), reads);
    h.job().state = "stopping";
    h.job().facts.root = { code: 0, signal: null, error: null };
    h.presentation.refresh();
    assert.match(plain(h.inspector()!.render(100)), /Cleanup: in progress/);
    assert.match(plain(h.inspector()!.render(100)), /root exit 0/);
    h.finishInspect();
    await opened;
    assert.equal(h.stops(), 0);
    assert.deepEqual(h.notifications, []);
  } finally { h.presentation.dispose(); }
});

test("picker stop requires explicit selection and confirmation, and checks fresh ownership/state", async () => {
  for (const scenario of ["approved", "cancel-action", "cancel-confirm", "unknown", "finished-during-confirm"] as const) {
    const h = harness();
    try {
      if (scenario === "unknown") { h.job().state = "unknown"; h.job().observationLoss = "historical"; }
      h.choose(async () => scenario === "cancel-action" ? undefined : "Stop job");
      h.confirm(async () => {
        if (scenario === "finished-during-confirm") h.job().state = "finished";
        return scenario !== "cancel-confirm";
      });
      await h.presentation.open(h.job().id);
      assert.equal(h.stops(), scenario === "approved" ? 1 : 0, scenario);
      if (scenario === "unknown") assert.deepEqual(h.choices[0], ["Inspect"]);
      if (scenario === "approved") assert.match(h.notifications[0] ?? "", /cleanup is not yet confirmed/);
    } finally { h.presentation.dispose(); }
  }
});

test("single shared refresh stops on host disposal, branch disposal and shutdown; late factories stay inert", (t) => {
  t.mock.timers.enable({ apis: ["setInterval"] });
  for (const disposal of ["surface", "tree", "shutdown"] as const) {
    const h = harness();
    h.presentation.refresh();
    h.presentation.refresh(); // Must not add a second interval.
    const reads = h.reads();
    t.mock.timers.tick(3000);
    assert.equal(h.reads(), reads + 3);
    const renderedAt = h.reads();
    h.widget()?.render(30);
    h.widget()?.render(100);
    assert.equal(h.reads(), renderedAt);
    if (disposal === "surface") h.widget()?.dispose?.();
    else h.presentation.dispose(disposal === "tree");
    t.mock.timers.tick(3000);
    assert.equal(h.reads(), renderedAt);
    if (disposal === "tree") assert.equal(h.widgetChanges.at(-1), undefined);
    if (disposal === "shutdown") assert.equal(h.widgetChanges.at(-1), JOBS_WIDGET); // Host owns shutdown surface reset.
    h.presentation.dispose(false);
  }
  const h = harness();
  h.deferWidget();
  h.presentation.refresh();
  const factory = h.factory();
  assert.ok(factory);
  h.presentation.dispose(false);
  const late = factory({ requestRender() {} } as TUI, theme);
  assert.deepEqual(late.render(100), []);
});

test("a failed display cannot fail job admission or escape a refresh timer", (t) => {
  t.mock.timers.enable({ apis: ["setInterval"] });
  const h = harness();
  h.breakWidget();
  assert.doesNotThrow(() => h.presentation.refresh());
  assert.match(h.notifications[0] ?? "", /Jobs display unavailable:.*fixture UI failure/);
  const reads = h.reads();
  t.mock.timers.tick(5000);
  assert.equal(h.reads(), reads);
  assert.equal(h.stops(), 0);
  h.presentation.dispose();
});

test("pending confirmation cannot stop after tree/shutdown disposal; open inspector is closed", async () => {
  const h = harness();
  let approved!: (yes: boolean) => void;
  h.choose(async () => "Stop job");
  h.confirm(() => new Promise((resolve) => { approved = resolve; }));
  const opened = h.presentation.open(h.job().id);
  await new Promise((r) => setImmediate(r));
  h.presentation.dispose();
  approved(true);
  await opened;
  assert.equal(h.stops(), 0);
  assert.deepEqual(h.notifications, []);

  const inspector = harness();
  inspector.choose(async () => "Inspect");
  const showing = inspector.presentation.open(inspector.job().id);
  await new Promise((r) => setImmediate(r));
  inspector.presentation.dispose(false);
  await showing;
  assert.equal(inspector.stops(), 0);
});

test("RPC, print/JSON and headless TUI create no widgets, pickers, refresh timers or model work", (t) => {
  t.mock.timers.enable({ apis: ["setInterval"] });
  for (const [mode, hasUI] of [["rpc", true], ["print", false], ["json", false], ["tui", false]] as const) {
    const h = harness(mode, hasUI);
    h.presentation.refresh();
    void h.presentation.open();
    t.mock.timers.tick(5000);
    assert.equal(h.reads(), 0);
    assert.equal(h.stops(), 0);
    assert.deepEqual(h.choices, []);
    assert.deepEqual(h.widgetChanges, []);
    h.presentation.dispose();
  }
});
