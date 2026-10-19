import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { createBashToolDefinition, createReadToolDefinition, initTheme, ToolExecutionComponent } from "@earendil-works/pi-coding-agent";
import { stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { registerUI as register } from "../extensions/ui/index.ts";
import { compactRenderers } from "./ui-fixture.mjs";
import { readExcerpt } from "../src/features/tools/read.ts";

initTheme("dark", false);
const plain = (lines) => stripTerminalSequences(lines.join("\n"));
function row(kind, args, requestRender = () => {}) {
  return new ToolExecutionComponent(kind, "fixture", args, { outputPad: 1 }, compactRenderers(kind), { requestRender }, "/missing/project");
}
function result(text, details, isError = false) { return { content: [{ type: "text", text }], details, isError }; }

test("selected builtins and otherwise unstyled tools share the compact shell", () => {
  let resolve;
  register({ registerCommand() {}, registerMarkdownTransformer() {}, on() {}, registerToolRenderer(fn) { resolve = fn; } });
  const original = {};
  for (const name of ["grep", "subagent", "mcp__lookup", "job_start"]) assert.equal(resolve(name, () => original).renderShell, "self");
  const custom = { renderCall() {} };
  assert.equal(resolve("mcp__lookup", () => custom), custom);
  for (const name of ["read", "write", "edit", "bash", "codemode"]) assert.equal(resolve(name, () => original).renderShell, "self");
});

test("bash stays compact while streaming, expands full command/output, records duration", () => {
  const component = row("bash", { command: "printf 'first\\n'\nprintf 'second\\n'" });
  component.markExecutionStarted();
  component.setArgsComplete();
  component.updateResult(result("first"), true);
  assert.match(plain(component.render(100)), /● bash/);
  assert.equal(component.render(100).filter((line) => line.trim()).length, 1);
  component.setExpanded(true);
  assert.match(plain(component.render(100)), /first/);
  assert.match(plain(component.render(100)), /second/);
  component.updateResult({ ...result("first\nsecond"), durationMs: 2100 });
  component.setExpanded(false);
  assert.match(plain(component.render(100)), /bash[\s\S]*second[^\n]*  2\.1s/);
  assert.doesNotMatch(plain(component.render(100)), /running/);
  assert.equal(component.render(100).filter((line) => line.trim()).length, 1);
});

test("silent bash calls expose their phase and expanded command before any output", () => {
  const component = row("bash", { command: "sleep 2\nprintf 'done\\n'" });
  assert.match(plain(component.render(100)), /· bash/);
  component.setArgsComplete();
  assert.match(plain(component.render(100)), /◦ bash/);
  component.markExecutionStarted();
  assert.match(plain(component.render(100)), /● bash/);
  component.setExpanded(true);
  assert.match(plain(component.render(100)), /1  sleep 2/);
  assert.match(plain(component.render(100)), /2  printf/);
  component.updateResult(result("done"));
  assert.doesNotMatch(plain(component.render(100)), /receiving|queued|running/);
  assert.match(plain(component.render(100)), /\n   bash/);
});

test("bash errors rely on isError, retain exit/timeout/abort diagnostics, never infer failure from stdout", () => {
  const component = row("bash", { command: "run-example" });
  component.updateResult(result("Command exited with code 42"));
  assert.match(plain(component.render(120)), /\n   bash/);
  for (const text of ["log output\n\nCommand exited with code 7", "log output\n\nCommand timed out after 3 seconds", "log output\n\nCommand aborted", "Command terminated without an exit code"]) {
    component.updateResult(result(text, undefined, true));
    const summary = plain(component.render(120));
    assert.match(summary, /✗/);
    assert.doesNotMatch(summary, /✓/);
    assert.match(summary, /exit 7|timed out|aborted|without an exit code/);
    component.setExpanded(true);
    for (const line of text.split("\n").filter(Boolean)) assert.ok(plain(component.render(120)).includes(line));
    component.setExpanded(false);
  }
});

test("truncation and full-output path stay visible when collapsed", () => {
  const component = row("bash", { command: "generate-output" });
  component.updateResult(result("tail", { truncation: { truncated: true }, fullOutputPath: "/tmp/example-output.log" }));
  const summary = plain(component.render(100));
  assert.match(summary, /truncated/);
  assert.match(summary, /Full output: \/tmp\/example-output.log/);
  component.updateResult(result("tail\n\n[Showing lines 3-4 of 4. Full output: /tmp/timeout-output.log]\n\nCommand timed out after 3 seconds", undefined, true));
  assert.match(plain(component.render(120)), /Full output: \/tmp\/timeout-output.log/);
  assert.match(plain(component.render(120)), /truncated/);
});

test("user-limited reads separate verified continuation notices and retain file line numbers", () => {
  const text = "const one = 1;\nconst two = 2;\n\n[8 more lines in file. Use offset=22 to continue.]";
  const args = { path: "example.ts", offset: 20, limit: 2 };
  const excerpt = readExcerpt(text, args);
  assert.equal(excerpt.source, "const one = 1;\nconst two = 2;");
  assert.equal(excerpt.start, 20);
  assert.match(excerpt.notice, /offset=22/);
  const component = row("read", args);
  component.updateResult(result(text));
  assert.match(plain(component.render(120)), /lines 20–21/);
  assert.match(plain(component.render(120)), /more available/);
  assert.doesNotMatch(plain(component.render(120)), /! |offset=22/);
  component.setExpanded(true);
  const output = plain(component.render(120));
  assert.match(output, /20  const one/);
  assert.match(output, /21  const two/);
  assert.equal(output.match(/offset=22/g).length, 1);
});

test("real partial reads preserve a selected blank final line, including blank-only excerpts", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "dots-read-blank-"));
  try {
    await writeFile(path.join(dir, "example.ts"), "const one = 1;\n\nconst three = 3;\n");
    const tool = createReadToolDefinition(dir);
    for (const args of [{ path: "example.ts", offset: 1, limit: 2 }, { path: "example.ts", offset: 2, limit: 1 }]) {
      const response = await tool.execute("fixture", args, undefined, undefined, {});
      const component = row("read", args);
      component.updateResult({ ...response, isError: false });
      assert.match(plain(component.render(100)), new RegExp(`lines ${args.offset}–2`));
      assert.match(plain(component.render(100)), /more available/);
      component.setExpanded(true);
      assert.ok(component.render(100).some((line) => /^\s*(?:│\s*)?2\s*$/.test(stripTerminalSequences(line))), "selected blank line 2 has a gutter");
    }
    // Whole-file EOF newlines must not gain a phantom numbered blank row.
    await writeFile(path.join(dir, "example.ts"), "const one = 1;\n");
    const args = { path: "example.ts" };
    const response = await tool.execute("fixture", args, undefined, undefined, {});
    const component = row("read", args);
    component.updateResult({ ...response, isError: false });
    component.setExpanded(true);
    assert.match(plain(component.render(100)), /lines 1–1/);
    assert.ok(!component.render(100).some((line) => /^\s*(?:│\s*)?2\s*$/.test(stripTerminalSequences(line))));
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test("notice-like source is preserved unless request and line counts match", () => {
  const source = "text\n\n[8 more lines in file. Use offset=99 to continue.]";
  assert.equal(readExcerpt(source, { limit: 1 }).source, source);
  assert.equal(readExcerpt(source, {}).source, source);
});

test("metadata-truncated reads show returned source and warnings, not fake source rows", () => {
  const content = "const value = 1;";
  const notice = "[Showing lines 40-40 of 90. Use offset=41 to continue.]";
  const component = row("read", { path: "example.ts", offset: 40 });
  component.updateResult(result(content + "\n\n" + notice, { truncation: { truncated: true, content, outputLines: 1 } }));
  assert.match(plain(component.render(120)), /truncated/);
  assert.match(plain(component.render(120)), /offset=41/);
  component.setExpanded(true);
  assert.match(plain(component.render(120)), /40  const value/);
  assert.equal(plain(component.render(120)).match(/offset=41/g).length, 1);
  component.updateResult(result("[Line 40 exceeds limit]", { truncation: { truncated: true, content: "", firstLineExceedsLimit: true } }));
  assert.match(plain(component.render(120)), /read limit exceeded/);
  assert.doesNotMatch(plain(component.render(120)), /40  \[/);
});

test("image blocks and their notes are left to Pi, not treated as numbered source", () => {
  const component = row("read", { path: "example.png" });
  const image = { content: [{ type: "text", text: "Read image file [image/png]\nImage dimensions: 1x1" }, { type: "image", mimeType: "image/png", data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a2ioAAAAASUVORK5CYII=" }], isError: false };
  const original = structuredClone(image);
  component.setShowImages(false);
  component.updateResult(image);
  assert.match(plain(component.render(120)), /image · preview hidden/);
  assert.match(plain(component.render(120)), /Image dimensions/);
  assert.doesNotMatch(plain(component.render(120)), /iVBOR|1  Read image/);
  assert.deepEqual(image, original);
  component.updateResult(result("Read image file [image/png]\nPreview unavailable"));
  assert.match(plain(component.render(120)), /Preview unavailable/);
});

test("read errors and empty results remain clear", () => {
  const component = row("read", { path: "example.ts" });
  component.updateResult(result(""));
  assert.match(plain(component.render(80)), /empty result/);
  component.updateResult(result("Access denied\nMore details", undefined, true));
  assert.match(plain(component.render(80)), /✗ read[\s\S]*Access denied/);
  component.setExpanded(true);
  assert.match(plain(component.render(80)), /More details/);
});

test("narrow layouts sanitize source/command/output and always fit", () => {
  for (const kind of ["bash", "read"]) {
    const component = row(kind, { command: "echo 👩‍💻\n\x1b[2Jbad", path: "漢字\x1b]0;title\x07.ts", offset: 999 });
    component.updateResult(result("漢字 👩‍💻 é\ttext\x1b[2J\nnext"));
    for (const expanded of [false, true]) {
      component.setExpanded(expanded);
      for (const width of [1, 2, 3, 5, 20, 40, 120]) {
        for (const line of component.render(width)) {
          assert.ok(visibleWidth(line) <= width);
          assert.doesNotMatch(line, /\x1b\[2J|\x1b\]0;/);
        }
      }
    }
  }
});

test("large bash display keeps the final diagnostic and reports omitted middle", () => {
  const component = row("bash", { command: "many-lines" });
  component.updateResult(result("first\n" + "middle\n".repeat(5000) + "last diagnostic"));
  component.setExpanded(true);
  const text = plain(component.render(80));
  assert.match(text, /first/);
  assert.match(text, /display limited/);
  assert.match(text, /last diagnostic/);
});

test("real SDK read and bash results match the renderer contracts", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "dots-compact-tools-"));
  try {
    await writeFile(path.join(dir, "example.ts"), "zero\none\ntwo\nthree\nfour\n");
    const args = { path: "example.ts", offset: 2, limit: 2 };
    const outcome = await createReadToolDefinition(dir).execute("read", args, undefined, undefined, {});
    const component = row("read", args);
    component.updateResult({ ...outcome, isError: false });
    component.setExpanded(true);
    const text = plain(component.render(100));
    assert.match(text, /2  one/);
    assert.match(text, /3  two/);
    assert.match(text, /offset=4/);
    const bash = createBashToolDefinition(dir, { exposeSessionEnvironment: false, operations: { async exec(_command, _cwd, options) { options.onData(Buffer.from("example failure\n")); return { exitCode: 7 }; } } });
    const failed = await bash.execute("bash", { command: "example-command" }, undefined, undefined, {});
    const bashRow = row("bash", { command: "example-command" });
    bashRow.updateResult(failed);
    assert.match(plain(bashRow.render(100)), /✗ bash[\s\S]*exit 7/);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
