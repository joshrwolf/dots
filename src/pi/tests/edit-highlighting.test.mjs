import assert from "node:assert/strict";
import test from "node:test";
import { initTheme, ToolExecutionComponent } from "@earendil-works/pi-coding-agent";
import { rgbColor, styleText, stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { highlight, DiffView, SourceView, editRenderers, writeRenderers } from "./ui-fixture.mjs";
import { pairLines, parsePatch, wordChanges } from "../src/ui/previews/patch.ts";


initTheme("dark", false);
const theme = {
  bold: (text) => `\x1b[1m${text}\x1b[22m`,
  appearance: "dark",
  fg: (_color, text) => text,
  style: (text, options) => styleText(text, options, "truecolor"),
  colors: { error: rgbColor(220, 70, 70), text: rgbColor(220, 220, 220), toolSuccessBg: rgbColor(20, 20, 20), toolErrorBg: rgbColor(20, 20, 20), toolDiffAdded: rgbColor(70, 180, 90), toolDiffRemoved: rgbColor(220, 70, 70), toolDiffContext: rgbColor(150, 150, 150) },
};
const plain = (lines) => stripTerminalSequences(lines.join("\n"));
const patch = parsePatch('@@ -1 +1,2 @@\n-  return "Hello, " + name;\n+  const displayName = name.trim() || "world";\n+  return `Hello, ${displayName}!`;\n');

test("pairs a return with its replacement, not the newly inserted declaration", () => {
  const rows = pairLines(patch.hunks[0].lines);
  assert.equal(rows.length, 2);
  assert.equal(rows[0].left, undefined);
  assert.match(rows[0].right.text, /const displayName/);
  assert.match(rows[1].left.text, /return/);
  assert.match(rows[1].right.text, /return/);
});

test("unrelated or oversized replacement blocks do not invent correspondence", () => {
  const lines = [{ kind: "removed", text: "alpha bravo" }, { kind: "added", text: "charlie delta" }];
  assert.equal(pairLines(lines).length, 2);
  const huge = Array.from({ length: 20 }, (_, i) => ({ kind: "removed", text: `const old${i} = 1;` })).concat(Array.from({ length: 20 }, (_, i) => ({ kind: "added", text: `const next${i} = 2;` })));
  assert.equal(pairLines(huge).length, 40);
});

test("word ranges isolate changed values and whitespace", () => {
  const before = "  timeout: 1000,";
  const after = "  timeout: 5000,";
  const changes = wordChanges(before, after);
  assert.deepEqual(changes.before.map((r) => before.slice(r.start, r.end)), ["1000"]);
  assert.deepEqual(changes.after.map((r) => after.slice(r.start, r.end)), ["5000"]);
  assert.equal(wordChanges("x".repeat(5000), "y").before.length, 0);
  assert.ok(wordChanges("a b", "a  b").after.length);
});

test("real Shiki tokens preserve source and distinguish template expressions", async () => {
  const source = 'const message = `Hello, ${name}!`;';
  const dark = await highlight(source, "sample.ts", "dark");
  const light = await highlight(source, "sample.ts", "light");
  assert.equal(dark[0].map((token) => token.content).join(""), source);
  assert.ok(new Set(dark[0].map((token) => token.color)).size >= 3);
  assert.notDeepEqual(dark, light);
  const name = dark[0].find((token) => token.content === "name");
  const text = dark[0].find((token) => token.content.includes("Hello"));
  assert.ok(name && text);
  assert.notEqual(name.color, text.color);
  const unknown = await highlight(source, "sample.unknown-language", "dark");
  assert.equal(unknown[0][0].content, source);
  assert.equal(unknown[0][0].color, undefined);
});

test("diff loading redraws once and caches highlighted layout across redraws", async () => {
  let redraws = 0;
  const view = new DiffView(patch, "sample.ts", theme, () => redraws++);
  const before = view.render(140);
  await view.prepare();
  assert.equal(redraws, 1);
  const after = view.render(140);
  assert.equal(plain(before), plain(after));
  assert.notDeepEqual(before, after);
  assert.strictEqual(view.render(140), after);
  const backgrounds = new Set(after.join("\n").match(/\x1b\[48;2;\d+;\d+;\d+m/g));
  assert.ok(backgrounds.size >= 3, "line and stronger word backgrounds");
  for (const width of [1, 5, 40, 80, 120, 140, 200]) {
    for (const line of view.render(width)) assert.ok(visibleWidth(line) <= width);
  }
  view.invalidate();
  view.render(140);
  await view.prepare();
  assert.equal(redraws, 1, "resize/invalidation does not retokenize");
});

test("async source highlighting follows the latest appearance without changing content", async () => {
  const source = 'const greeting = `Hello, ${name}!`;\n';
  let redraws = 0;
  const view = new SourceView(source, "sample.ts", theme, () => redraws++);
  const dark = view.prepare();
  view.setTheme({ ...theme, appearance: "light" });
  const light = view.prepare();
  await Promise.all([dark, light]);
  assert.equal(redraws, 1, "stale dark completion must not redraw");
  const lines = view.render(100);
  assert.match(plain(lines), /Hello/);
  assert.match(lines.join("\n"), /\x1b\[38;2;/);
  const expected = await highlight(source, "sample.ts", "light");
  assert.ok(expected[0].some((token) => token.color));
  for (const width of [1, 3, 20, 80]) for (const line of view.render(width)) assert.ok(visibleWidth(line) <= width);
});

test("unicode changed words preserve graphemes and reject terminal control sequences", async () => {
  const parsed = parsePatch('@@ -1 +1 @@\n-const emoji = "👩‍💻 é";\n+const emoji = "👨‍💻 ê";\n');
  const view = new DiffView(parsed, "sample.ts", theme);
  await view.prepare();
  const output = plain(view.render(140));
  assert.match(output, /👩‍💻 é/);
  assert.match(output, /👨‍💻 ê/);
  const source = new SourceView('const x = "\x1b[2Junsafe";', "sample.ts", theme);
  await source.prepare();
  assert.doesNotMatch(source.render(80).join("\n"), /\x1b\[2J/);
});

function writeRow(content) {
  const row = new ToolExecutionComponent("write", "write-1", { path: "sample.ts", content }, { outputPad: 1 }, writeRenderers(), { requestRender() {} }, "/missing/project");
  row.setArgsComplete();
  row.updateResult({ content: [{ type: "text", text: "Successfully wrote file" }], isError: false });
  return row;
}

test("write uses compact summaries and a single-column source preview, not a fabricated diff", () => {
  const row = writeRow('const one = 1;\nconst two = 2;\n');
  assert.equal(row.render(140).filter((line) => line.trim()).length, 1);
  assert.match(plain(row.render(140)), /write +sample\.ts +2 lines/);
  row.setExpanded(true);
  const expanded = plain(row.render(140));
  assert.match(expanded, /1  const one/);
  assert.match(expanded, /2  const two/);
  assert.doesNotMatch(expanded, /before|after|@@/);
  row.updateResult({ content: [{ type: "text", text: "Permission denied\nCheck access" }], isError: true });
  row.setExpanded(false);
  assert.match(plain(row.render(140)), /✗ write[\s\S]*Permission denied/);
  row.setExpanded(true);
  assert.match(plain(row.render(140)), /Check access/);
  assert.doesNotMatch(plain(row.render(140)), /const one/);
});

test("write progress follows streamed arguments, execution, and completion without timers", () => {
  const row = new ToolExecutionComponent("write", "stream", { path: "sample.ts", content: "const" }, { outputPad: 1 }, writeRenderers(), { requestRender() {} }, "/missing/project");
  assert.match(plain(row.render(120)), /· write +sample\.ts +~5 chars/);
  row.updateArgs({ path: "sample.ts", content: "x".repeat(4200) });
  assert.match(plain(row.render(120)), /· write +sample\.ts +~4\.2k chars/);
  row.setArgsComplete();
  assert.match(plain(row.render(120)), /◦ write/);
  row.markExecutionStarted();
  assert.match(plain(row.render(120)), /● write/);
  row.updateResult({ content: [{ type: "text", text: "Successfully wrote file" }], isError: false });
  assert.match(plain(row.render(120)), /\n   write/);
  assert.doesNotMatch(plain(row.render(120)), /receiving|writing…|queued/);
});

test("empty writes, incomplete arguments, and oversized source remain bounded", () => {
  const empty = writeRow("");
  assert.match(plain(empty.render(80)), /0 lines/);
  const partial = new ToolExecutionComponent("write", "partial", {}, undefined, writeRenderers(), { requestRender() {} }, "/missing");
  partial.setExpanded(true);
  assert.doesNotMatch(plain(partial.render(80)), /written|failed/);
  const huge = writeRow("x".repeat(500_001));
  huge.setExpanded(true);
  assert.match(plain(huge.render(80)), /display limit/);
});

test("compact summaries color meaning, not paths or write line counts", () => {
  const colors = { text: 37, toolDiffAdded: 32, toolDiffRemoved: 31, success: 32, error: 31, dim: 90, muted: 90, toolTitle: 37 };
  const palette = { ...theme, fg: (role, text) => `\x1b[${colors[role]}m${text}\x1b[39m` };
  for (const name of ["edit", "write"]) {
    const renderer = name === "edit" ? editRenderers() : writeRenderers();
    const context = { state: {}, args: { path: "sample.ts", content: "one\ntwo\n" }, argsComplete: true, expanded: false, isError: false, outputPad: 1, invalidate() {} };
    renderer.renderCall(context.args, palette, context);
    const result = { content: [{ type: "text", text: "Done" }], details: { patch: "@@ -1 +1 @@\n-old\n+new\n" } };
    const body = renderer.renderResult(result, { expanded: false, isPartial: false }, palette, context);
    const output = body.render(120).join("\n");
    assert.ok(output.includes("\x1b[37msample.ts"), JSON.stringify(output));
    if (name === "edit") {
      assert.ok(output.includes("\x1b[32m+1\x1b[39m"));
      assert.match(output, /\x1b\[31m−1/);
    } else {
      assert.ok(output.includes("\x1b[90m2 lines"), JSON.stringify(output));
      assert.doesNotMatch(plain([output]), /written|✓/);
      assert.match(output, /\x1b\[1m/);
    }
    for (const width of [1, 3, 10, 20, 40]) for (const line of body.render(width)) assert.ok(visibleWidth(line) <= width);
    const failed = renderer.renderResult({ content: [{ type: "text", text: "Denied" }] }, { expanded: false, isPartial: false }, palette, { ...context, isError: true });
    assert.match(plain(failed.render(120)), /✗[\s\S]*Denied/);
    assert.doesNotMatch(plain(failed.render(120)), /failed/);
    assert.doesNotMatch(failed.render(120).join("\n"), /\x1b\[48;2;/);
    assert.doesNotMatch(failed.render(120).join("\n"), /\x1b\[32m/);
  }
});
