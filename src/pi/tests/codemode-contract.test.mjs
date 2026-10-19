import assert from "node:assert/strict";
import test from "node:test";
import { createCodemodeExtension, createReadToolDefinition, initTheme, ToolExecutionComponent } from "@earendil-works/pi-coding-agent";
import { stripTerminalSequences } from "@earendil-works/pi-tui";
import { compactRenderers } from "./ui-fixture.mjs";

// Exercise the real VM and live publisher through public SDK APIs. Tool outcomes
// are hand-authored: no filesystem access, external calls, or session captures.
test("native codemode publishes the summaries consumed by live and replay presentation", { timeout: 20_000 }, async () => {
  initTheme("dark", false);
  let tool;
  createCodemodeExtension({ models: false })({ registerTool(value) { tool = value; }, getAllTools: () => [] });
  const args = { code: '// @options: {"timeout_ms": 10000}\nawait Promise.allSettled([tools.read({path:"example.ts"}), tools.read({path:"missing.ts"})]); text("script finished");' };
  const component = new ToolExecutionComponent("codemode", "contract", args, { outputPad: 1 }, compactRenderers("codemode"), { requestRender() {} }, "/workspace");
  const updates = [];
  let next = 0;
  const result = await tool.execute("contract", args, undefined, (update) => {
    updates.push(structuredClone(update));
    component.updateResult({ ...update, isError: false }, true);
    assert.match(stripTerminalSequences(component.render(120).join("\n")), /● codemode/);
  }, {
    tools: [createReadToolDefinition("/workspace")], sessionManager: { getBranch: () => [] },
    async executeTool(name, args) {
      const isError = args.path === "missing.ts";
      return { toolCall: { id: `contract/${++next}`, name, arguments: args }, isError,
        result: { content: [{ type: "text", text: isError ? "ENOENT: missing.ts" : "hand-authored source" }], details: {} } };
    },
  });
  assert.ok(updates.some((update) => update.details.calls.some((call) => call.status === "running")));
  assert.deepEqual(result.details.calls.map((call) => call.status).sort(), ["error", "ok"]);
  assert.ok(result.details.calls.every((call) => typeof call.args === "string" && Number.isFinite(call.durationMs)));
  assert.equal(result.isError, undefined); // allSettled catches the child failure.
  component.updateResult({ ...result, isError: false });
  const rendered = stripTerminalSequences(component.render(120).join("\n"));
  assert.match(rendered, /\n   codemode/);
  assert.match(rendered, /1 done · 1 failed/);
  assert.match(rendered, /ENOENT: missing.ts/);
  assert.match(rendered, /script finished/);
  assert.doesNotMatch(rendered, /hand-authored source/); // Nested results are not in the record.
});
