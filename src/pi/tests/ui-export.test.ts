import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { SessionManager, type Theme } from "@earendil-works/pi-coding-agent";
import { Text } from "@earendil-works/pi-tui";
import { toolRenderers } from "../extensions/ui/index.ts";
import { createSyntaxService, createToolRenderers, type ToolObservation } from "@joshrwolf/pi-dots/ui";
// Private imports are confined to this exact-version host-contract test.
import { exportSessionToHtml } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/export-html/index.js";
import { createToolHtmlRenderer } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/export-html/tool-renderer.js";
import { getThemeByName } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

test("real export bypasses builtin templates but invokes a standalone foundation consumer", async () => {
  const dir = await mkdtemp(join(tmpdir(), "dots-ui-export-"));
  const syntax = createSyntaxService();
  try {
    const names = ["bash", "read", "write", "edit", "ls", "example", "codemode"];
    const session = SessionManager.create(dir, dir);
    session.appendMessage({ role: "user", content: "Hand-authored UI fixture", timestamp: 1 });
    session.appendMessage({
      role: "assistant", content: names.map((name) => ({ type: "toolCall", id: name, name, arguments: { path: "example.ts", command: "example", content: "fixture", code: "text(42);" } })),
      api: "anthropic-messages", provider: "anthropic", model: "example", stopReason: "toolUse", timestamp: 2,
      usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
    });
    for (const name of names) session.appendMessage({ role: "toolResult", toolCallId: name, toolName: name, content: [{ type: "text", text: `Recorded ${name} result` }], isError: false, timestamp: 3, ...(name === "codemode" ? { details: { calls: [{ id: "nested", name: "read", args: '{"path":"example.ts"}', status: "ok" }] } } : {}) });
    const visited: string[] = [];
    const renderers = createToolRenderers({
      shell: "self",
      project: (frame: ToolObservation) => frame.slot === "call" ? "FOUNDATION_INVOCATION" : "FOUNDATION_RESULT",
      create: () => {
        const component = new Text("", 0, 0);
        return {
          update(model: string, theme: Theme) { component.setText(theme.fg("success", model)); },
          invalidate() { component.invalidate(); },
          render: (width: number) => component.render(width),
        };
      },
    });
    const theme = getThemeByName("dark");
    assert.ok(theme);
    const renderer = createToolHtmlRenderer({ getToolRenderers(name) { visited.push(name); return name === "codemode" ? toolRenderers(syntax).get(name) : renderers; }, theme, cwd: dir });
    const output = await exportSessionToHtml(session, undefined, { outputPath: join(dir, "export.html"), toolRenderer: renderer });
    const html = await readFile(output, "utf8");
    assert.ok(visited.length > 0);
    assert.ok(visited.every((name) => name === "example" || name === "codemode"));
    const encoded = /<script id="session-data" type="application\/json">([^<]+)<\/script>/.exec(html)?.[1];
    assert.ok(encoded, "export contains embedded session data");
    const payload = Buffer.from(encoded, "base64").toString("utf8");
    assert.match(payload, /FOUNDATION_INVOCATION/);
    assert.match(payload, /FOUNDATION_RESULT/);
    assert.match(payload, /Recorded write result/);
    assert.ok(visited.includes("codemode"));
    const expanded = JSON.parse(payload).renderedTools.codemode.resultHtmlExpanded as string;
    const exportedText = expanded.replace(/<[^>]+>/g, "").replace(/│/g, " ").replace(/\s+/g, " ");
    assert.match(exportedText, /nested results are not recorded/);
    assert.match(payload, /Arguments/);
    assert.doesNotMatch(payload, />running</, "the serialized invocation must not claim a job is still running");
  } finally { syntax.dispose(); await rm(dir, { recursive: true, force: true }); }
});
