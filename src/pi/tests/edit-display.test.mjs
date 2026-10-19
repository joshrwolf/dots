import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { createEditToolDefinition, initTheme, ToolExecutionComponent } from "@earendil-works/pi-coding-agent";
import { rgbColor, styleText, stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { registerUI as register } from "../extensions/ui/index.ts";
import { editRenderers, writeRenderers, DiffView, SourceView } from "./ui-fixture.mjs";
import { MAX_PATCH_CHARS, pairLines, parsePatch, wordChanges } from "../src/ui/previews/patch.ts";
import { sourceText as clean } from "../src/ui/text.ts";

initTheme("dark", false);
const theme = {
  bold: (text) => `\x1b[1m${text}\x1b[22m`,
  appearance: "dark",
  fg: (_color, text) => text,
  style: (text, options) => styleText(text, options, "truecolor"),
  colors: { error: rgbColor(220, 70, 70), text: rgbColor(220, 220, 220), toolSuccessBg: rgbColor(20, 20, 20), toolErrorBg: rgbColor(20, 20, 20), toolDiffAdded: rgbColor(70, 180, 90), toolDiffRemoved: rgbColor(220, 70, 70), toolDiffContext: rgbColor(150, 150, 150) },
};
const patch = "--- sample.ts\n+++ sample.ts\n@@ -1,3 +1,4 @@\n const before = 1;\n-const old = 2;\n+const next = 3;\n+const extra = 4;\n const after = 5;\n";
const result = { content: [{ type: "text", text: "Successfully replaced text in sample.ts." }], details: { patch }, isError: false };
const plain = (lines) => lines.map(stripTerminalSequences).join("\n");
function row(renderers = editRenderers(), args = { path: "sample.ts" }) {
  return new ToolExecutionComponent("edit", "edit-1", args, { outputPad: 1 }, renderers, { requestRender() {} }, "/missing/project");
}
function assertFits(lines, width) {
  for (const line of lines) assert.ok(visibleWidth(line) <= width, `${visibleWidth(line)} > ${width}: ${JSON.stringify(line)}`);
}

test("validates hunks and preserves both coordinate systems and blank lines", () => {
  const parsed = parsePatch(patch);
  assert.equal(parsed.added, 2);
  assert.equal(parsed.removed, 1);
  assert.deepEqual(parsed.hunks[0].lines.at(-1), { kind: "context", text: "const after = 5;", old: 3, next: 4 });
  const empty = parsePatch("@@ -0,0 +1 @@\n+\n");
  assert.deepEqual(empty.hunks[0].lines, [{ kind: "added", text: "", next: 1 }]);
  const deletion = parsePatch("@@ -1 +0,0 @@\n-last\n\\ No newline at end of file\n");
  assert.equal(deletion.removed, 1);
  assert.equal(deletion.hunks[0].lines.at(-1).kind, "note");
});

test("accepts multiple hunks and handles source resembling patch headers", () => {
  const parsed = parsePatch("--- a\n+++ a\n@@ -1 +1 @@\n--- old\n+++ new\n@@ -20 +20 @@\n-x\n+y\n");
  assert.equal(parsed.hunks.length, 2);
  assert.equal(parsed.added, 2);
  assert.equal(parsed.hunks[0].lines[0].text, "-- old");
});

test("rejects malformed, incomplete, multifile, or oversized patches", () => {
  for (const source of ["", "nonsense", "@@ -1,2 +1 @@\n-a\n+b\n", "@@ -1 +1 @@\n-a\n+b\n+c\n", patch + patch, "@@ -99999999999999999999 +1 @@\n-a\n+b\n", "x".repeat(MAX_PATCH_CHARS + 1)]) {
    assert.equal(parsePatch(source), undefined);
  }
});

test("pairing preserves unequal blocks, order, and explicit newline notes", () => {
  const rows = pairLines(parsePatch(patch).hunks[0].lines);
  assert.equal(rows[1].left.text, "const old = 2;");
  assert.equal(rows[1].right.text, "const next = 3;");
  assert.equal(rows[2].left, undefined);
  assert.equal(rows[2].right.text, "const extra = 4;");
});

test("registers only presentation; preserves other custom renderers", () => {
  let resolver;
  register({ registerCommand() {}, registerMarkdownTransformer() {}, on() {}, registerToolRenderer(fn) { resolver = fn; } });
  const downstream = { renderShell: "default", renderCall() {} };
  for (const tool of ["find", "custom_edit"]) {
    assert.equal(resolver(tool, () => downstream), downstream);
  }
  assert.equal(resolver("edit", () => downstream).renderShell, "self");
  assert.equal(resolver("write", () => downstream).renderShell, "self");
});

test("real host component transitions from pending to one compact summary", () => {
  const component = row();
  assert.match(plain(component.render(120)), /edit.*sample\.ts/);
  component.markExecutionStarted();
  component.setArgsComplete();
  component.updateResult(result);
  const collapsed = component.render(140);
  assert.equal(collapsed.filter((line) => line.trim()).length, 1);
  assert.match(plain(collapsed), /edit\s+sample\.ts +\+2 −1/);
  assert.doesNotMatch(plain(collapsed), /const old/);
  component.setExpanded(true);
  assert.match(plain(component.render(140)), /before.*│ after/);
  assert.match(plain(component.render(140)), /const old = 2;.*│.*const next = 3;/);
  assert.doesNotMatch(plain(component.render(80)), /before.*│ after/);
  assert.ok(component.render(80).every((line) => (stripTerminalSequences(line).match(/│/g) ?? []).length <= 1));
  assert.match(plain(component.render(80)), /const extra = 4;/);
  component.setExpanded(false);
  assert.equal(component.render(140).filter((line) => line.trim()).length, 1);
});

test("errors remain visible collapsed; expanded errors preserve full details", () => {
  const component = row();
  component.updateResult({ content: [{ type: "text", text: "Unique match not found\nTry a larger context." }], isError: true });
  assert.match(plain(component.render(80)), /✗ edit[\s\S]*Unique match not found/);
  component.setExpanded(true);
  assert.match(plain(component.render(80)), /Try a larger context/);
  assert.doesNotMatch(plain(component.render(80)), /applied/);
});

test("missing and malformed patches fall back to recorded output without rereading files", () => {
  for (const details of [{ diff: "-1 old\n+1 new" }, { patch: "not a patch" }, undefined]) {
    const component = row();
    component.updateResult({ ...result, details });
    component.setExpanded(true);
    const rendered = plain(component.render(100));
    assert.match(rendered, /preview unavailable/);
    assert.match(rendered, /old|not a patch|Successfully replaced/);
  }
});

test("replay requires only recorded arguments/results and invalidation updates themes", () => {
  const component = row();
  component.updateResult(result);
  component.setExpanded(true);
  const first = component.render(140);
  initTheme("light", false);
  component.invalidate();
  const second = component.render(140);
  assert.equal(plain(first), plain(second));
  assert.notDeepEqual(first, second);
  const replay = row();
  replay.updateResult(result);
  replay.setExpanded(true);
  assert.deepEqual(replay.render(140), second);
  initTheme("dark", false);
});

test("all layouts fit tiny to wide widths, Unicode, tabs, controls, and padding", () => {
  const text = "\t漢字 👩‍💻 é \x1b[2J\x1b]0;bad\x07 \u202eend";
  const source = `@@ -1 +1 @@\n-${text}\n+${text}!\n`;
  const component = row(undefined, { path: text + "\nfile.ts" });
  component.updateResult({ ...result, details: { patch: source } });
  for (const expanded of [false, true]) {
    component.setExpanded(expanded);
    for (const padding of [0, 1]) {
      component.setOutputPad(padding);
      for (const width of [1, 2, 3, 5, 10, 40, 80, 119, 120, 121, 140, 240]) {
        const lines = component.render(width);
        assertFits(lines, width);
        assert.doesNotMatch(lines.join("\n"), /\x1b\[2J|\x1b\]0;|\u202e/);
      }
    }
  }
  assert.equal(clean("a\r\x00b\t"), "ab    ");
});

test("long source uses unified view and wraps rather than silently clipping", () => {
  const long = "identifier ".repeat(30) + "TAIL";
  const parsed = parsePatch(`@@ -1 +1 @@\n-${long}\n+short\n`);
  const view = new DiffView(parsed, "sample.txt", theme);
  const lines = view.render(140);
  assert.doesNotMatch(plain(lines), /│/);
  assert.match(plain(lines), /TAIL/);
  assertFits(lines, 140);
  assert.strictEqual(view.render(140), lines, "warm redraw reuses layout");
  view.invalidate();
  assert.notStrictEqual(view.render(140), lines);
});

test("large diffs have an explicit display limit", () => {
  const source = `@@ -0,0 +1,2000 @@\n${Array.from({ length: 2000 }, (_, n) => `+line ${n}`).join("\n")}\n`;
  const lines = new DiffView(parsePatch(source), "sample.txt", theme).render(80);
  assert.equal(lines.length, 1000);
  assert.match(plain(lines), /display limited/);
});

test("independent call/result slot rendering retains the completed summary", () => {
  const renderers = editRenderers();
  const context = { state: {}, args: { path: "sample.ts" }, expanded: false, isError: false, outputPad: 1, invalidate() {} };
  const call = plain(renderers.renderCall(context.args, theme, context).render(100));
  assert.match(call, /sample\.ts/);
  assert.doesNotMatch(call, /editing|applied/);
  const collapsed = plain(renderers.renderResult(result, { expanded: false, isPartial: false }, theme, context).render(100));
  assert.match(collapsed, /\+2 −1/);
  const expanded = plain(renderers.renderResult(result, { expanded: true, isPartial: false }, theme, context).render(100));
  assert.match(expanded, /const next/);
});

test("actual SDK edit result renders multi-edits and leaves execution untouched", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "dots-edit-display-"));
  try {
    await writeFile(path.join(dir, "sample.txt"), "one\ntwo\nthree\nfour\n");
    const args = { path: "sample.txt", edits: [{ oldText: "one", newText: "ONE" }, { oldText: "four", newText: "FOUR" }] };
    const tool = createEditToolDefinition(dir);
    const outcome = await tool.execute("actual", args, undefined, undefined, {});
    assert.ok(parsePatch(outcome.details.patch));
    const component = row(undefined, args);
    component.updateResult({ ...outcome, isError: false });
    component.setExpanded(true);
    assert.match(plain(component.render(140)), /ONE/);
    assert.match(plain(component.render(140)), /FOUR/);
    assert.equal(await readFile(path.join(dir, "sample.txt"), "utf8"), "ONE\ntwo\nthree\nFOUR\n");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
