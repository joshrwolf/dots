import assert from "node:assert/strict";
import test, { after } from "node:test";
import { initTheme, ToolExecutionComponent } from "@earendil-works/pi-coding-agent";
import { ProcessTerminal, TuiMainScreen, stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { toolRenderers } from "../extensions/ui/index.ts";
import { createSyntaxService } from "../src/index.ts";
import { projectCodemode } from "../src/features/tools/codemode.ts";
import { parsePreferences } from "../src/host/preferences.ts";
import type { ToolObservation } from "../src/host/tools.ts";

initTheme("dark", false);
const syntax = createSyntaxService();
after(() => syntax.dispose());
const plain = (lines: string[]) => stripTerminalSequences(lines.join("\n"));
const header = { type: "text" as const, text: "Script completed\nWall time 0.1 seconds\nOutput:\n" };
const call = (name: string, status: string, extra: object = {}) => ({ id: "parent/?", name, args: '{"path":"example.ts"}', status, ...extra });
function row() {
  return new ToolExecutionComponent("codemode", "fixture", { code: 'const answer = 42;\ntext(answer);' }, { outputPad: 1 }, toolRenderers(syntax).get("codemode"), new TuiMainScreen(new ProcessTerminal()), "/workspace");
}
function frame(details: unknown): Extract<ToolObservation, { slot: "result" }> {
  return { slot: "result", args: {}, details, content: [header], partial: false, error: false, durationMs: 1, showImages: false };
}

test("codemode transitions through live summaries and replays the same recorded result", () => {
  const live = row();
  assert.match(plain(live.render(100)), /codemode/);
  live.setArgsComplete();
  assert.match(plain(live.render(100)), /codemode/);
  live.markExecutionStarted();
  assert.doesNotMatch(plain(live.render(100)), /running/); // Export retains this neutral invocation slot.
  live.updateResult({ content: [], details: { calls: [call("read", "running"), call("bash", "running")] }, isError: false }, true);
  assert.match(plain(live.render(100)), /2 running/);
  const result = { content: [header, { type: "text" as const, text: "answer: 42" }], details: { calls: [call("read", "ok"), call("bash", "error", { error: "exit 7" }), call("models.classify", "cancelled")] }, isError: false };
  live.updateResult(result);
  const replay = row(); replay.updateResult(structuredClone(result));
  assert.equal(plain(live.render(100)), plain(replay.render(100)));
  assert.match(plain(live.render(100)), /\n   codemode/); // A caught child error is not a parent failure.
  assert.match(plain(live.render(100)), /bash: exit 7/);
  assert.match(plain(live.render(100)), /1 done · 1 failed · 1 cancelled/);
  assert.match(plain(live.render(100)), /answer: 42/);
  assert.doesNotMatch(plain(live.render(100)), /Script completed/);
  live.setExpanded(true);
  const expanded = plain(live.render(120));
  assert.match(expanded, /nested results are not recorded/);
  assert.match(expanded, /Arguments \(preview\)/);
  assert.match(expanded, /const answer = 42/);
  for (let width = 1; width <= 100; width++) for (const line of live.render(width)) assert.ok(visibleWidth(line) <= width, `${width}: ${line}`);
});

test("collapsed calls prioritize old errors over newer successes; paths and failure tails remain visible", () => {
  const component = row();
  component.updateResult({ content: [header, { type: "text", text: "ordinary output\n".repeat(20) + "Script error: refused" }],
    details: { calls: [call("early-failure", "error", { error: "permission denied" }), ...Array.from({ length: 12 }, (_, n) => call(`success-${n}`, "ok"))], fullOutputPath: "/tmp/example-output.txt" }, isError: true });
  const rendered = plain(component.render(120));
  assert.match(rendered, /early-failure: permission denied/);
  assert.match(rendered, /Script error: refused/);
  assert.match(rendered, /Full output: \/tmp\/example-output.txt/);
  assert.match(rendered, /9 more calls/);
  assert.match(rendered, /expand for output/);
});

test("malformed, oversized, and absent summaries remain honest and bounded", () => {
  assert.equal(projectCodemode(frame(undefined)).available, false);
  assert.equal(projectCodemode(frame({ calls: [] })).available, true);
  const model = projectCodemode(frame({ calls: [null, call("unknown", "new-status"), call("bad-args", "ok", { args: {} }), call("safe", "ok", { cost: NaN, durationMs: -1 })] }));
  assert.equal(model.unreadable, 3);
  assert.equal(model.calls[0]?.cost, undefined);
  assert.equal(model.calls[0]?.durationMs, undefined);
  const large = projectCodemode(frame({ calls: Array.from({ length: 300 }, () => call("example", "ok", { args: "x".repeat(10_000) })) }));
  assert.equal(large.total, 300);
  assert.equal(large.calls.length, 256);
  assert.ok(large.calls[0]!.args.length < 2100);
  const component = row();
  component.updateResult({ content: [], details: { calls: Array.from({ length: 300 }, () => call("example", "ok")) }, isError: false });
  assert.match(plain(component.render(120)), /counts below cover this sample only/);
  component.setExpanded(true);
  assert.ok(component.render(1).length < 3000);
});

test("recorded argument previews stay opaque and reported costs survive collapsed selection", () => {
  const args = '{"path":"abbreviated…';
  assert.equal(projectCodemode(frame({ calls: [call("read", "ok", { args })] })).calls[0]?.args, args);
  const component = row();
  component.updateResult({ content: [], details: { calls: Array.from({ length: 6 }, () => call("models.classify", "ok", { cost: 0.01 })) }, isError: false });
  assert.match(plain(component.render(120)), /reported cost \$0\.0600/);
  component.updateResult({ content: [], details: { calls: [] }, isError: false });
  assert.match(plain(component.render(120)), /0 calls/);
});

test("only the exact native header block is removed; existing explicit preferences stay explicit", () => {
  const input = { ...frame({ calls: [] }), content: [{ type: "text" as const, text: "Script completed\nuser output" }] };
  assert.equal(projectCodemode(input).output, "Script completed\nuser output");
  assert.deepEqual(parsePreferences({ version: 1, tools: ["bash"] }).tools, ["bash"]);
  assert.ok(parsePreferences({ version: 1 }).tools.includes("codemode"));
  assert.deepEqual(parsePreferences({ version: 1, tools: ["codemode"] }).tools, ["codemode"]);
});
