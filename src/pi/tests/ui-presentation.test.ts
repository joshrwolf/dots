import assert from "node:assert/strict";
import { mkdtemp, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { CustomEditor, ToolExecutionComponent, getSelectListTheme, initTheme, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { ProcessTerminal, TuiMainScreen, stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { activityDecoration, activityHeader, Notices, StateGutter, type ActivityState, DiffView, parsePatch, PriorityRow, ActivityCard, SoftEditor, createSyntaxService, fitLabel, type RowModel } from "@joshrwolf/pi-dots/ui";
import { duration } from "../src/features/tools/records.ts";
import { toolRenderers } from "../extensions/ui/index.ts";
import { installEditor } from "../src/features/editor.ts";
import { configureUI } from "../src/features/settings.ts";
import { defaultPreferences, loadPreferences, parsePreferences, savePreferences, type Preferences } from "../src/host/preferences.ts";
// Exact-version host fixtures; production imports public APIs only.
import { KeybindingsManager } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/keybindings.js";
import { getThemeByName } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

const plain = (lines: string[]) => stripTerminalSequences(lines.join("\n"));
initTheme("dark", false);
const theme = getThemeByName("dark");
assert.ok(theme);

test("middle clipping preserves filename tails and grapheme-safe width bounds", () => {
  const path = "packages/日本語/👩🏽‍💻/a-long-directory/report.ts";
  assert.match(fitLabel(path, 24, "middle"), /report\.ts$/);
  for (let width = 0; width < 100; width++) {
    const result = fitLabel(path, width, "middle");
    assert.ok(visibleWidth(result) <= width, `${width}: ${result}`);
    assert.doesNotMatch(result, /\uFFFD/);
  }
});

test("inline and edge layouts differ only in placement, not shrink decisions", () => {
  const model: RowModel = {
    start: [{ text: "src/日本語/report.ts", tone: "toolTitle", priority: 40, shrink: "clip", minWidth: 10, clip: "middle" }],
    end: [
      { text: "failed", tone: "error", priority: 100, shrink: "clip", minWidth: 6 },
      { text: "12.3s", tone: "dim", priority: 0, shrink: "omit" },
    ], endSeparator: " · ",
  };
  const inline = new PriorityRow();
  const edges = new PriorityRow();
  const implicit = new PriorityRow();
  inline.update({ ...model, layout: "inline" }, theme);
  edges.update({ ...model, layout: "edges" }, theme);
  implicit.update(model, theme);
  for (const width of [...Array.from({ length: 100 }, (_, n) => n), 240]) {
    const left = plain(inline.render(width));
    const right = plain(edges.render(width));
    assert.ok(visibleWidth(left) <= width);
    assert.ok(visibleWidth(right) <= width);
    assert.equal(left.trim().replace(/\s+/g, " "), right.trim().replace(/\s+/g, " "));
    assert.equal(plain(implicit.render(width)), right);
    if (width >= 6) assert.match(left, /failed/);
    if (width <= 10) assert.doesNotMatch(left, /·/);
  }
  assert.equal(plain(inline.render(240)).trimEnd(), "src/日本語/report.ts failed · 12.3s");
  assert.equal(visibleWidth(plain(edges.render(240)).trimEnd()), 240);
});

test("both layouts handle missing groups without orphan separators", () => {
  const item = { text: "value", tone: "dim", priority: 1, shrink: "omit" } as const;
  for (const layout of ["inline", "edges"] as const) {
    const row = new PriorityRow();
    for (const [start, end] of [[[], []], [[item], []], [[], [item]]] as const) {
      row.update({ layout, start, end, separator: " · ", endSeparator: " / " }, theme);
      for (const width of [1, 5, 40]) {
        const rendered = plain(row.render(width));
        assert.doesNotMatch(rendered, /[·/]/);
        assert.ok(visibleWidth(rendered) <= width);
        assert.equal(rendered.trim(), width >= 5 && (start.length || end.length) ? "value" : "");
        if (layout === "inline" && rendered.trim()) assert.ok(rendered.startsWith("value"));
      }
    }
  }
});

test("ActivityCard uses one explicit header model and preserves body width", () => {
  const card = new ActivityCard();
  const widths: number[] = [];
  const model = {
    header: { layout: "inline", start: [{ text: "write example.ts", tone: "toolTitle", priority: 50, shrink: "clip" }],
      end: [{ text: "written", tone: "success", priority: 100, shrink: "clip" }] } as const,
    notices: [], caption: "recorded source",
    body: { invalidate() {}, render(width: number) { widths.push(width); return ["body"]; } },
  };
  card.update(model, theme);
  assert.equal(plain(card.render(240)).split("\n")[0]?.trimEnd(), "  write example.ts written");
  card.update({ ...model, header: { ...model.header, layout: "edges" } }, theme);
  assert.equal(visibleWidth(plain(card.render(240)).split("\n")[0]!.trimEnd()), 240);
  assert.deepEqual(widths, [238, 238]);
});

test("group separators belong to surviving groups in both layouts", () => {
  for (const layout of ["inline", "edges"] as const) {
    const row = new PriorityRow();
    const start = [{ text: "read", tone: "muted", priority: 10, shrink: "omit" }] as const;
    const end = [{ text: "done", tone: "success", priority: 100, shrink: "clip", minWidth: 4 }] as const;
    row.update({ layout, start, end, groupSeparator: " · " }, theme);
    assert.match(plain(row.render(60)), /^read · +done/);
    for (let width = 1; width <= 10; width++) {
      const rendered = plain(row.render(width));
      assert.doesNotMatch(rendered, /·/);
      assert.ok(visibleWidth(rendered) <= width);
    }
    for (const groups of [{ start: [], end }, { start, end: [] }, { start: [], end: [] }]) {
      row.update({ layout, ...groups, groupSeparator: " · " }, theme);
      assert.doesNotMatch(plain(row.render(60)), /·/);
    }
  }
});

test("soft slots align short labels, never cap long labels, and yield before content", () => {
  const row = new PriorityRow();
  for (const layout of ["inline", "edges"] as const) {
    for (const label of ["$", "read", "write", "codemode", "日本語"]) {
      const start = [{ text: label, tone: "muted", priority: 90, shrink: "clip", padTo: 5 },
        { text: "done", tone: "success", priority: 100, shrink: "clip", minWidth: 4 }] as const;
      row.update({ layout, start, end: [], separator: " · " }, theme);
      assert.equal(plain(row.render(60)).trimEnd(), label + " ".repeat(Math.max(0, 5 - visibleWidth(label))) + " · done");
      const tight = visibleWidth(label) + 3 + 4;
      assert.equal(plain(row.render(tight)).trim(), `${label} · done`);
    }
  }
  row.update({ layout: "inline", start: [{ text: "", tone: "dim", priority: 0, shrink: "clip", padTo: 50 }], end: [] }, theme);
  assert.equal(plain(row.render(60)).trim(), "");
});

test("explicit card headers preserve caller ordering and spacing", () => {
  const card = new ActivityCard();
  card.update({ header: { layout: "inline", separator: " · ", groupSeparator: " · ",
    start: [{ text: "read", tone: "muted", priority: 90, shrink: "clip", padTo: 5 }, { text: "done", tone: "success", priority: 100, shrink: "clip" }],
    end: [{ text: "example.ts", tone: "toolTitle", priority: 10, shrink: "clip" }],
  }, body: undefined, notices: [], caption: "" }, theme);
  assert.equal(plain(card.render(120)).trimEnd(), "  read  · done · example.ts");
});

test("segment separators express related metadata without orphan punctuation", () => {
  const row = new PriorityRow();
  for (const layout of ["inline", "edges"] as const) {
    row.update({ layout, separator: " · ", start: [
      { text: "+3", tone: "toolDiffAdded", priority: 10, shrink: "omit" },
      { text: "−2", tone: "toolDiffRemoved", priority: 100, shrink: "clip", separatorBefore: " " },
    ], end: [] }, theme);
    assert.equal(plain(row.render(40)).trim(), "+3 −2");
    assert.equal(plain(row.render(2)).trim(), "−2");
  }
});

test("short timings are quiet only in collapsed tool summaries", () => {
  const state = { kind: "result", text: "", partial: false, error: false, durationMs: 42, image: false, showImages: false } as const;
  assert.deepEqual(duration(state), []);
  assert.equal(duration(state, true)[0]?.text, "0.0s");
  assert.equal(duration({ ...state, durationMs: 1000 })[0]?.text, "1.0s");
  assert.deepEqual(duration({ ...state, partial: true }, true), []);
});

test("caption policy follows actual header visibility on every resize", () => {
  const card = new ActivityCard();
  const path = "/workspace/example.ts";
  const model = { header: { start: [{ text: path, tone: "toolTitle", priority: 10, shrink: "clip" }], end: [] },
    caption: path, notices: [], body: { invalidate() {}, render() { return ["SOURCE"]; } },
  } as const;
  card.update({ ...model, captionPolicy: "when-not-in-header" }, theme);
  const wide = plain(card.render(100));
  assert.equal(wide.split(path).length - 1, 1);
  assert.equal(wide.split("\n").length, 2); // No empty caption row.
  const narrow = card.render(12).slice(1).map((line) => stripTerminalSequences(line).replace(/^│ /, "").trim()).join("");
  assert.ok(narrow.includes(path));
  assert.equal(plain(card.render(100)), wide);
  card.update(model, theme);
  assert.equal(plain(card.render(100)).split(path).length - 1, 2); // Explicit/default always.
});

test("split diffs size to content, grow for long lines, and retain fill as an option", () => {
  const syntax = createSyntaxService();
  try {
    const patch = parsePatch("--- example.ts\n+++ example.ts\n@@ -1 +1 @@\n-old\n+日本語\n");
    assert.ok(patch);
    const options = { patch, path: "example.ts", theme, syntax };
    const content = new DiffView(options);
    const fill = new DiffView({ ...options, widthPolicy: "fill" });
    const divider = (view: DiffView) => visibleWidth(plain(view.render(240)).split("│")[0]!);
    assert.ok(divider(content) < 30);
    assert.ok(divider(fill) > 100);
    assert.doesNotMatch(plain(content.render(80)), /│/);
    const longPatch = parsePatch("--- example.ts\n+++ example.ts\n@@ -1 +1 @@\n-" + "a".repeat(80) + "\n+" + "b".repeat(100) + "\n");
    assert.ok(longPatch);
    const long = new DiffView({ ...options, patch: longPatch });
    assert.ok(divider(long) > divider(content));
    for (const view of [content, fill, long]) for (const width of [1, 20, 80, 120, 240]) {
      assert.ok(view.render(width).every((line) => visibleWidth(line) <= width));
    }
  } finally { syntax.dispose(); }
});

test("preferred cell widths align sanitized labels and still yield by priority", () => {
  const row = new PriorityRow();
  for (const layout of ["inline", "edges"] as const) {
    for (const label of ["web", "example-api", "a-much-longer-label", "日本語", " web\t "]) {
      row.update({ layout, start: [
        { text: "?", tone: "warning", priority: 100, shrink: "clip", minWidth: 1 },
        { text: label, tone: "toolTitle", priority: 90, shrink: "clip", width: 12, padTo: 30 },
        { text: "asking", tone: "dim", priority: 50, shrink: "clip" },
      ], end: [] }, theme);
      const wide = plain(row.render(80));
      assert.equal(visibleWidth(wide.slice(0, wide.indexOf("asking"))), 15);
      assert.ok(visibleWidth(wide) <= 80);
      const narrow = plain(row.render(10));
      assert.match(narrow, /^\?/);
      assert.doesNotMatch(narrow, /asking/);
      assert.ok(visibleWidth(narrow) <= 10);
    }
    row.update({ layout, start: [
      { text: "a long subject", tone: "muted", priority: 10, shrink: "clip", width: 12, minWidth: 8 },
    ], end: [{ text: "failed", tone: "error", priority: 100, shrink: "clip", minWidth: 6 }] }, theme);
    assert.equal(plain(row.render(6)).trim(), "failed");
  }
});

test("row indentation is layout-owned and stays bounded in narrow panes", () => {
  const row = new PriorityRow();
  for (const layout of ["inline", "edges"] as const) {
    for (const indent of [0, 4]) {
      row.update({ layout, indent, start: [
        { text: "  ⤷", tone: "muted", priority: 100, shrink: "clip", minWidth: 1 },
        { text: "twig", tone: "toolTitle", priority: 50, shrink: "clip" },
      ], end: [] }, theme);
      assert.equal(plain(row.render(40)).indexOf("⤷"), indent);
      assert.deepEqual(row.render(0), []);
      for (const width of [1, 2, 4, 5, 8, 40]) {
        const lines = row.render(width);
        assert.equal(lines.length, 1);
        assert.ok(visibleWidth(plain(lines)) <= width);
        assert.equal(plain(lines).indexOf("⤷"), Math.min(indent, width - 1));
      }
    }
  }
});

test("state decoration is static, distinct without color, and bounded at tiny widths", () => {
  const child = { invalidate() {}, render: (width: number) => ["bash".slice(0, width)] };
  const expected: Record<ActivityState, string> = { receiving: "·", queued: "◦", running: "●", done: " ", failed: "✗", cancelled: "−", unknown: "?", waiting: "?" };
  for (const state of Object.keys(expected) as ActivityState[]) {
    const view: StateGutter = new StateGutter(child, state, theme);
    assert.equal(plain(view.render(20)), `${expected[state]} bash`);
    assert.deepEqual(view.render(20), view.render(20));
    for (const width of [0, 1, 2, 3, 20]) assert.ok(view.render(width).every((line) => visibleWidth(line) <= width));
  }
});

test("activity colors distinguish work from attention without coloring every success", () => {
  assert.deepEqual(activityDecoration("running"), { glyph: "●", tone: "accent" });
  assert.deepEqual(activityDecoration("waiting"), { glyph: "?", tone: "warning" });
  assert.deepEqual(activityDecoration("failed"), { glyph: "✗", tone: "error" });
  assert.deepEqual(activityDecoration("done"), { glyph: " ", tone: "muted" });
  const notices = new Notices();
  notices.update([{ text: "Recover the result", severity: "warning" }, { text: "Permission denied", severity: "error" }], theme);
  assert.match(plain(notices.render(80)), /! Recover the result/);
  assert.match(plain(notices.render(80)), /↳ Permission denied/);
});

test("bold identity and failure badges retain text and never overflow", () => {
  const row = new PriorityRow();
  for (const appearance of ["light", "dark"]) {
    const palette = getThemeByName(appearance);
    assert.ok(palette);
    row.update({ layout: "inline", start: [
      { text: "bash", tone: "toolTitle", bold: true, priority: 90, shrink: "clip" },
      { text: "failed", tone: "error", badge: "error", minWidth: 8, priority: 100, shrink: "clip" },
    ], end: [] }, palette);
    assert.match(plain(row.render(80)), /bash +failed/);
    assert.match(row.render(80).join(""), /\x1b\[1m/);
    assert.match(row.render(80).join(""), /\x1b\[48;/);
    for (let width = 0; width < 80; width++) assert.ok(row.render(width).every((line) => visibleWidth(line) <= width));
  }
});

test("tool subjects have a stable anchor and diagnostics are softly indented", () => {
  const card = new ActivityCard();
  const render = (verb: string, metadata: string) => {
    card.update({ header: activityHeader(verb, "example.ts", [{ text: metadata, tone: "dim", priority: 50, shrink: "clip" }]),
      state: "done", notices: [], body: undefined, caption: "" }, theme);
    return plain(card.render(120));
  };
  const write = render("write", "2 lines");
  const read = render("read", "lines 1–3");
  assert.equal(write.indexOf("example.ts"), read.indexOf("example.ts"));
  assert.match(write, /write +example\.ts +2 lines/);
  assert.doesNotMatch(write, /·/);
  card.update({ header: activityHeader("read", "missing.ts", []), state: "failed",
    notices: [{ text: "File not found", severity: "error" }], body: undefined, caption: "" }, theme);
  const lines = card.render(120).map((line) => stripTerminalSequences(line));
  assert.match(lines[0]!, /^✗ read/);
  assert.match(lines[1]!, /^  ↳ File not found/);
  assert.doesNotMatch(lines.join(""), /failed/);
  for (const width of [1, 8, 16, 20, 80]) assert.ok(card.render(width).every((line) => visibleWidth(line) <= width));
});

test("row shrink priority is independent of whether a segment declares a floor", () => {
  const row = new PriorityRow();
  for (const minWidth of [undefined, 8]) {
    row.update({ start: [
      { text: "?", tone: "warning", priority: 100, shrink: "clip" },
      { text: "example-web", tone: "toolTitle", priority: 90, shrink: "clip", ...(minWidth === undefined ? {} : { minWidth }) },
      { text: "asking: Which ingress?", tone: "dim", priority: 50, shrink: "clip" },
    ], end: [] }, theme);
    const rendered = plain(row.render(30));
    assert.match(rendered, /\? example-web asking:/);
    assert.doesNotMatch(rendered, /Which ingress\?/);
    assert.ok(visibleWidth(rendered) <= 30);
  }
});

test("explicit floors protect pass one and yield by priority in pass two", () => {
  const row = new PriorityRow();
  row.update({ start: [
    { text: "abcdefghij", tone: "muted", priority: 10, shrink: "clip", minWidth: 4 },
    { text: "0123456789", tone: "toolTitle", priority: 90, shrink: "clip" },
  ], end: [] }, theme);
  const first = plain(row.render(10));
  const [low, high] = first.trim().split(" ");
  assert.ok(low && high);
  assert.equal(visibleWidth(low), 4); // Four-cell floor survives the first pass.
  assert.equal(visibleWidth(high), 5);
  assert.match(low, /^a/);
  assert.match(high, /^0/);
  assert.ok(visibleWidth(first) <= 10);
  row.update({ start: [
    { text: "abcdefghij", tone: "muted", priority: 10, shrink: "clip", minWidth: 4 },
    { text: "failed", tone: "error", priority: 100, shrink: "clip", minWidth: 6 },
  ], end: [] }, theme);
  assert.equal(plain(row.render(6)).trim(), "failed");
});

test("layout owns optional separators and preserves outcomes ahead of desired subject widths", () => {
  const row = new PriorityRow();
  row.update({
    start: [{ text: "very-long-subject.ts", tone: "toolTitle", priority: 40, shrink: "clip", minWidth: 16, clip: "middle" }],
    end: [
      { text: "low-priority", tone: "dim", priority: 0, shrink: "omit" },
      { text: "failed", tone: "error", priority: 100, shrink: "clip", minWidth: 6 },
    ], endSeparator: " · ",
  }, theme);
  for (const width of [8, 16, 25]) {
    const text = plain(row.render(width));
    assert.match(text, /failed/);
    assert.doesNotMatch(text, /·/);
    assert.ok(visibleWidth(text) <= width);
  }
});

test("file subjects are cwd-relative, full paths stay in expanded details, errors wrap separately", () => {
  const syntax = createSyntaxService();
  try {
    const path = "/workspace/packages/example/deep/module.ts";
    const component = new ToolExecutionComponent("read", "fixture", { path }, { outputPad: 1 }, toolRenderers(syntax).get("read"), new TuiMainScreen(new ProcessTerminal()), "/workspace");
    component.updateResult({ content: [{ type: "text", text: "const value = 1;" }], isError: false });
    assert.doesNotMatch(plain(component.render(42)), /\/workspace/);
    assert.match(plain(component.render(42)), /module\.ts/);
    component.setExpanded(true);
    const expanded = component.render(30).map((line) => stripTerminalSequences(line).replace(/^\s*│ ?/, "").trim()).join("");
    assert.ok(expanded.includes(path));
    component.updateResult({ content: [{ type: "text", text: "Permission denied: choose a writable destination." }], isError: true });
    component.setExpanded(false);
    assert.match(plain(component.render(60)), /✗ read[\s\S]*Permission denied/);
    for (const width of [1, 3, 20, 42, 80, 140]) assert.ok(component.render(width).every((line) => visibleWidth(line) <= width));
  } finally { syntax.dispose(); }
});

test("soft editor preserves native text, cursor layout, controls, and semantic border callback", () => {
  const tui = new TuiMainScreen(new ProcessTerminal());
  const bindings = new KeybindingsManager({});
  const editorTheme = { borderColor: (text: string) => theme.fg("warning", text), selectList: getSelectListTheme() };
  const native = new CustomEditor(tui, editorTheme, bindings, { paddingX: 1, embedWorkingStatus: true });
  const soft = new SoftEditor(tui, editorTheme, bindings);
  for (const editor of [native, soft]) {
    editor.focused = true;
    editor.setText("日本語 👩🏽‍💻\nsecond line");
    editor.handleInput("\x1b[D");
    editor.handleInput("!");
  }
  assert.equal(soft.getText(), native.getText());
  for (const width of [8, 20, 80]) {
    assert.equal(plain(soft.render(width)), plain(native.render(width)));
    assert.ok(soft.render(width).every((line) => visibleWidth(line) <= width));
  }
  for (const width of [1, 2, 3]) {
    assert.equal(plain(soft.render(width)), "…");
    assert.ok(soft.render(width).every((line) => visibleWidth(line) <= width));
  }
  assert.equal(soft.getText(), native.getText());
  const mouse = { type: "click" as const, button: "left" as const, x: 2, y: 1, screenX: 2, screenY: 1, width: 80, height: 5, shift: false, alt: false, ctrl: false };
  assert.equal(soft.handleMouse({ ...mouse, width: 1 }), undefined);
  assert.deepEqual(soft.handleMouse(mouse), native.handleMouse(mouse));
  assert.equal(plain(soft.render(80)), plain(native.render(80)));
  const border = (text: string) => theme.fg("success", text);
  soft.borderColor = border;
  assert.doesNotMatch(soft.render(80).join("\n"), /\x1b\[2m/);
  native.borderColor = border;
  assert.deepEqual(soft.render(80), native.render(80));
  assert.equal(soft.borderColor, border);
  let escaped = 0;
  let exited = 0;
  soft.onEscape = () => escaped++;
  soft.handleInput("\x1b");
  soft.setText("");
  soft.onCtrlD = () => exited++;
  soft.handleInput("\x04");
  assert.equal(escaped, 1);
  assert.equal(exited, 1);
  assert.equal(soft.embedWorkingStatus, true);
});

test("editor installation and cleanup respect previous and subsequent owners", () => {
  type Factory = ReturnType<ExtensionContext["ui"]["getEditorComponent"]>;
  let factory: Factory;
  let installs = 0;
  const host = { mode: "tui" as const, ui: { getEditorComponent: () => factory, setEditorComponent(value: Factory) { factory = value; installs++; } } };
  const release = installEditor(host);
  assert.equal(installs, 1);
  assert.ok(factory);
  const ours = factory;
  const other: NonNullable<Factory> = (tui, theme, keys) => new CustomEditor(tui, theme, keys);
  factory = other;
  release(); release();
  assert.equal(factory, other);
  installEditor(host)();
  assert.equal(installs, 1);
  factory = undefined;
  const clean = installEditor(host);
  assert.notEqual(factory, ours);
  clean(); clean();
  assert.equal(factory, undefined);
  assert.equal(installs, 3);
  installEditor({ ...host, mode: "rpc" })();
  assert.equal(installs, 3);
});

test("settings cancel is side-effect free; apply saves before reload", async () => {
  let saved: Preferences | undefined;
  const events: string[] = [];
  const choices = ["Use native UI", "Cancel", "Use native UI", "Apply and reload"];
  const ui = { async select() { return choices.shift(); }, notify(text: string) { assert.fail(text); } };
  const store = { load: () => defaultPreferences, save(value: Preferences, expected: Preferences) { assert.equal(expected, defaultPreferences); saved = value; events.push("save"); } };
  const reload = async () => { events.push("reload"); };
  await configureUI(ui, reload, store);
  assert.equal(saved, undefined);
  assert.deepEqual(events, []);
  await configureUI(ui, reload, store);
  assert.deepEqual(saved, { version: 1, tools: [], otherTools: "native", thinking: "native", header: "native", footer: "native", editor: "native" });
  assert.deepEqual(events, ["save", "reload"]);
});

test("settings shutdown cancels a pending selector and defeats a queued Apply", async () => {
  for (const race of [false, true]) {
    const controller = new AbortController();
    let selectCalls = 0;
    let choose: ((value: string | undefined) => void) | undefined;
    const ui = {
      select(_title: string, _items: string[], options?: { signal?: AbortSignal }): Promise<string | undefined> {
        selectCalls++;
        return new Promise((resolve) => {
          choose = resolve;
          options?.signal?.addEventListener("abort", () => resolve(undefined), { once: true });
        });
      },
      notify() { assert.fail("retired UI must not be notified"); },
    };
    const pending = configureUI(ui, async () => { assert.fail("reload after cancellation"); }, {
      load: () => defaultPreferences,
      save() { assert.fail("save after cancellation"); },
    }, controller.signal);
    assert.ok(choose);
    if (race) choose("Apply and reload");
    controller.abort();
    await pending;
    assert.equal(selectCalls, 1);
  }
});

test("reload failure is reported by the host, not through an invalidated settings UI", async () => {
  const ui = {
    async select() { return "Apply and reload"; },
    notify() { assert.fail("old UI used after reload"); },
  };
  await assert.rejects(configureUI(ui, async () => { throw new Error("replacement failed"); }, {
    load: () => defaultPreferences,
    save() {},
  }), /replacement failed/);
});

test("preference persistence is private, validated, and detects intervening edits", async () => {
  const dir = await mkdtemp(join(tmpdir(), "dots-ui-preferences-"));
  const path = join(dir, "preferences.json");
  try {
    const next = parsePreferences({ version: 1, tools: ["read"], editor: "native" });
    savePreferences(next, defaultPreferences, path);
    assert.deepEqual(loadPreferences(path), next);
    assert.equal((await stat(path)).mode & 0o777, 0o600);
    assert.throws(() => savePreferences(defaultPreferences, defaultPreferences, path), /changed while/);
    assert.deepEqual(loadPreferences(path), next);
    assert.throws(() => parsePreferences({ version: 1, header: null }), /header/);
    assert.throws(() => parsePreferences({ version: 1, header: "unknown" }), /header/);
    assert.throws(() => parsePreferences({ version: 1, editor: null }), /editor/);
    assert.throws(() => parsePreferences({ version: 1, editor: "unknown" }), /editor/);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
