import assert from "node:assert/strict";
import test from "node:test";
import { initTheme } from "@earendil-works/pi-coding-agent";
import { ProcessTerminal, TuiMainScreen, VStack, stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { createSyntaxService, Inspector, OutputView } from "../src/index.ts";
import { FooterView, createHeader } from "../src/features/frame.ts";
import { scenarios } from "../dev/scenarios.ts";
import { paintPreviewLine } from "../dev/surface.ts";
import { Workbench, type WorkbenchState } from "../dev/workbench.ts";
// Version-pinned host fixtures; no private imports in production or the workbench.
import { KeybindingsManager } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/keybindings.js";
import { getThemeByName } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

initTheme("dark", false);
const dark = getThemeByName("dark")!;
const light = getThemeByName("light")!;
const bindings = new KeybindingsManager({});
const tui = new TuiMainScreen(new ProcessTerminal());
const plain = (lines: string[]) => stripTerminalSequences(lines.join("\n"));

function syntaxFixture() {
  return createSyntaxService(async () => ({ load: async () => {}, tokenize: (source) => source.split("\n").map((content) => [{ content, offset: 0 }]), dispose() {} }));
}

test("every workbench scenario uses real bounded components in both themes and expansion states", () => {
  const syntax = syntaxFixture();
  try {
    for (const scenario of scenarios) {
      const view = scenario.create({ syntax, tui, bindings, close() {}, redraw() {} }, dark);
      assert.ok(view.component instanceof VStack, `${scenario.id}: keep native layout identity`);
      for (const theme of [dark, light]) for (const expanded of [false, true]) for (let phase = 0; phase < scenario.phases.length; phase++) {
        view.update(phase, expanded, theme);
        for (const width of [1, 2, 3, 8, 20, 60, 120, 240]) {
          const lines = view.component.render(width);
          assert.ok(lines.length < 5000, `${scenario.id}: bounded preparation/output`);
          assert.ok(lines.every((line) => visibleWidth(line) <= width), `${scenario.id}/${phase}/${width}: line overflow`);
          assert.doesNotMatch(plain(lines), /\uFFFD|\x07|\x1b/);
        }
      }
    }
  } finally { syntax.dispose(); }
});

test("goal starts with nested groups and cycles every fixture in one scenario", () => {
  assert.equal(scenarios.filter((scenario) => scenario.id.startsWith("goal")).length, 1);
  const h = harness("goal");
  try {
    for (const expected of [/Core interface/, /Core interface/, /totals unavailable/, /Update exports/,
      /Resolve compatibility/, /4\/4 slices done/, /3 followed/, /Goal source unavailable/,
      /Loading goal/, /Brief truncated/, /No slices recorded/]) {
      assert.match(plain(h.view.render(160)), expected);
      h.view.handleInput(" ");
    }
    assert.equal(h.view.state.phase, 0);
    assert.match(plain(h.view.render(160)), /Core interface/);
  } finally { h.dispose(); }
});

test("native inspector handles scroll/close without taking ownership of actions", () => {
  let closed = 0;
  let redraws = 0;
  const inspector = new Inspector(bindings, () => closed++, () => redraws++);
  inspector.setContent({ title: "Recorded output", body: new OutputView("line\n".repeat(100)) }, dark);
  for (const key of ["\x1b[A", "\x1b[B", "\x1b[5~", "\x1b[6~"]) inspector.handleInput(key);
  assert.equal(redraws, 4);
  inspector.handleInput("s");
  assert.equal(redraws, 4);
  inspector.handleInput("\x1b");
  assert.equal(closed, 1);
});

test("inspector scrolling works through legacy render calls and keeps live refresh position", () => {
  let height = 5;
  const inspector = new Inspector(bindings, () => {}, () => {}, () => height);
  const model = { title: "Output", body: new OutputView(Array.from({ length: 100 }, (_, i) => `line ${i}`).join("\n")) };
  inspector.setContent(model, dark);
  assert.match(plain(inspector.render(60)), /line 0/);
  inspector.handleInput("\x1b[F");
  assert.match(plain(inspector.render(60)), /line 99/);
  assert.doesNotMatch(plain(inspector.render(60)), /line 0\b/);
  inspector.setContent(model, light);
  assert.match(plain(inspector.render(60)), /line 99/);
  inspector.handleInput("\x1b[H");
  inspector.render(60);
  const result = inspector.handleMouse({ type: "wheel", button: "none", x: 1, y: 2, screenX: 1, screenY: 2, width: 60, height: 7, shift: false, ctrl: false, alt: false, wheelDelta: 7 });
  assert.equal(result?.handled, true);
  assert.match(plain(inspector.render(60)), /line 7/);
  height = 2;
  assert.equal(inspector.render(60).length, 4);
  inspector.setContent(model, dark, true);
  assert.match(plain(inspector.render(60)), /line 0/);
});

test("workbench shortcuts accept uppercase and terminal-protocol keys", () => {
  for (const [next, previous] of [["n", "p"], ["N", "P"], ["\x1b[110u", "\x1b[112u"], ["\x1b[110;2u", "\x1b[112;2u"]]) {
    const h = harness();
    try {
      h.view.handleInput(next!);
      assert.equal(h.view.state.scenario, "read", JSON.stringify(next));
      h.view.handleInput(previous!);
      assert.equal(h.view.state.scenario, "bash", JSON.stringify(previous));
      h.view.handleInput("E");
      assert.equal(h.view.state.expanded, true);
    } finally { h.dispose(); }
  }
});

function harness(initial = "bash") {
  const syntax = syntaxFixture();
  let theme = dark;
  let closed = 0;
  let redraws = 0;
  let confirm: (signal: AbortSignal) => Promise<boolean> = async () => false;
  const changes: WorkbenchState[] = [];
  const view = new Workbench({ syntax, tui, bindings, theme: () => theme, close: () => closed++, redraw: () => redraws++,
    toggleTheme: () => { theme = theme === dark ? light : dark; }, confirmStop: (signal) => confirm(signal), changed: (state) => changes.push(state),
  }, initial);
  return { view, changes, closed: () => closed, redraws: () => redraws, confirm: (fn: typeof confirm) => { confirm = fn; }, dispose: () => { view.dispose(); syntax.dispose(); } };
}

test("workbench advances actual tool states explicitly and retains failures collapsed", () => {
  const h = harness();
  try {
    assert.deepEqual(h.view.state, { scenario: "bash", phase: 0, expanded: false });
    for (let n = 0; n < 5; n++) h.view.handleInput(" ");
    assert.equal(h.view.state.phase, 5);
    const collapsed = plain(h.view.render(120));
    assert.match(collapsed, /exit 1/);
    assert.match(collapsed, /Full output: \/fixture\/output\/test.log/);
    assert.doesNotMatch(collapsed, /expected visible diagnostic/);
    h.view.handleInput("e");
    assert.match(plain(h.view.render(120)), /expected visible diagnostic/);
    const before = h.view.render(120);
    h.view.handleInput("t");
    assert.notDeepEqual(h.view.render(120), before);
    assert.equal(plain(h.view.render(120)), plain(before));
    h.view.handleInput("n");
    assert.equal(h.view.state.scenario, "read");
    assert.equal(h.view.state.phase, 0);
    h.view.handleInput("\x1b");
    assert.equal(h.closed(), 1);
  } finally { h.dispose(); }
});

test("simulated job actions require approval and cannot act on stale or retired views", async () => {
  for (const condition of ["approve", "cancel", "dispose", "switch", "unknown", "reject"] as const) {
    const h = harness("job");
    try {
      let answer: (value: boolean) => void = () => { assert.fail("unexpected answer"); };
      let pendingSignal: AbortSignal | undefined;
      h.confirm((signal) => { pendingSignal = signal; return new Promise((resolve, reject) => {
        answer = resolve;
        if (condition === "reject") reject(new Error("dialog unavailable"));
      }); });
      if (condition === "unknown") for (let n = 0; n < 5; n++) h.view.handleInput(" ");
      const pending = h.view.requestStop();
      if (condition === "unknown") { assert.equal(pendingSignal, undefined); await pending; continue; }
      assert.ok(pendingSignal);
      if (condition === "dispose") { h.view.dispose(); assert.equal(pendingSignal.aborted, true); }
      if (condition === "switch") h.view.handleInput("n");
      answer(condition !== "cancel");
      await pending;
      const text = plain(h.view.render(160));
      if (condition === "approve") { assert.match(text, /State: stopping/); assert.equal(h.view.state.phase, 2); }
      else assert.doesNotMatch(text, /State: stopping/);
      if (condition === "reject") assert.match(text, /Confirmation failed/);
      if (condition === "dispose") {
        const count = h.redraws();
        h.view.handleInput(" ");
        assert.equal(h.redraws(), count);
        assert.equal(text, "");
      }
    } finally { h.dispose(); }
  }
});

test("preview resets return to the page, preserving explicit child backgrounds and RGB payloads", () => {
  for (const theme of [dark, light]) {
    const reset = paintPreviewLine("left\x1b[0mright", 9, theme);
    const prefix = paintPreviewLine("", 0, theme).replace(/\x1b\[0m$/, "");
    const fg = theme.getFgAnsi("text");
    assert.ok(reset.includes("\x1b[0m" + fg), "full reset restores the page foreground");
    assert.equal(plain([reset]), "leftright");
    const bg = /\x1b\[48;[^m]*m/.exec(prefix)?.[0];
    assert.ok(bg);
    assert.ok(reset.includes("\x1b[0m" + fg + bg));
    assert.ok(paintPreviewLine("a\x1b[22;49mb", 2, theme).includes("\x1b[22;49m" + bg));
    const child = "\x1b[48;2;0;39;49mselected\x1b[49m";
    assert.ok(paintPreviewLine(child, 8, theme).includes(child + bg), "RGB channels are not interpreted as resets");
    const compound = "\x1b[0;48;2;10;20;30m";
    assert.ok(paintPreviewLine(compound + "x", 1, theme).includes(compound + fg + "x"), "explicit background following reset wins");
    assert.ok(paintPreviewLine("\x1b[48:2::10:20:30mx", 1, theme).includes("\x1b[48:2::10:20:30mx"));
    for (const width of [8, 40, 100]) assert.equal(visibleWidth(paintPreviewLine(child, width, theme)), width);
  }
});

test("B jumps to the frame footer without changing its data or expansion state", () => {
  for (const key of ["b", "B", "\x1b[98u", "\x1b[98;2u"]) {
    const h = harness("frame");
    try {
      h.view.handleInput(" ");
      h.view.render(120); // Establish native scroll bounds before jumping to bottom.
      const before = h.view.state;
      h.view.handleInput(key);
      assert.deepEqual(h.view.state, before);
      assert.match(plain(h.view.render(120)), /✦ high.*! ctx 80%/);
      h.view.handleInput("t");
      assert.match(plain(h.view.render(120)), /✦ high.*! ctx 80%/);
      h.view.handleInput("n");
      assert.equal(h.view.state.scenario, "bash");
    } finally { h.dispose(); }
  }
});

test("frame presentation is deterministic, theme-owned, and shows unknown context honestly", () => {
  const footer = new FooterView();
  for (const theme of [dark, light]) {
    assert.equal(plain(createHeader(theme).render(100)).trimEnd(), "Pi  /ui appearance");
    footer.update({ project: "日本語 👩🏽‍💻", model: "example-model", contextPercent: undefined, statuses: [] }, theme);
    assert.match(plain(footer.render(100)), /ctx \?/);
    footer.update({ project: "example", model: "example-model", contextPercent: 95, statuses: ["workers 1"] }, theme);
    for (let width = 1; width < 100; width++) assert.ok(footer.render(width).every((line) => visibleWidth(line) <= width));
    assert.match(plain(footer.render(8)), /ctx 95%/);
  }
});
