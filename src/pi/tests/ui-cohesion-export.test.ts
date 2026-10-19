import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { SessionManager } from "@earendil-works/pi-coding-agent";
import { rendererResolver } from "../extensions/ui/index.ts";
import { createSyntaxService } from "../src/index.ts";
import { jobFixture } from "../dev/scenarios.ts";
import { summary } from "../extensions/jobs/protocol.ts";
import { jobOutput } from "../extensions/jobs/results.ts";
// Exact-version host contract: a cold export has no live runtime or TUI row.
import { exportSessionToHtml } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/export-html/index.js";
import { createToolHtmlRenderer } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/export-html/tool-renderer.js";
import { getThemeByName } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

test("cold HTML export uses registered background presentation and retains expanded evidence", async () => {
  const directory = await mkdtemp(join(tmpdir(), "dots-cohesion-export-"));
  const syntax = createSyntaxService();
  try {
    const session = SessionManager.create(directory, directory);
    const job = jobFixture(4);
    session.appendMessage({ role: "assistant", content: [{ type: "toolCall", id: "job", name: "job_status", arguments: { id: job.id } }],
      api: "anthropic-messages", provider: "anthropic", model: "example", stopReason: "toolUse", timestamp: 1,
      usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
    });
    session.appendMessage({ role: "toolResult", toolCallId: "job", toolName: "job_status", content: [{ type: "text", text: summary(job) }], details: { jobs: [jobOutput(job)] }, isError: false, timestamp: 2 });
    const resolve = rendererResolver(syntax);
    const theme = getThemeByName("dark");
    assert.ok(theme);
    const renderer = createToolHtmlRenderer({ getToolRenderers: (name) => resolve(name, () => undefined), theme, cwd: directory });
    const output = await exportSessionToHtml(session, undefined, { outputPath: join(directory, "export.html"), toolRenderer: renderer });
    const html = await readFile(output, "utf8");
    const encoded = /<script id="session-data" type="application\/json">([^<]+)<\/script>/.exec(html)?.[1];
    assert.ok(encoded);
    const payload = JSON.parse(Buffer.from(encoded, "base64").toString("utf8"));
    const expanded = String(payload.renderedTools.job.resultHtmlExpanded).replace(/<[^>]+>/g, "").replace(/│/g, " ").replace(/\s+/g, " ");
    assert.match(expanded, /exit 1/);
    assert.match(expanded, /Capture incomplete: 1024 bytes dropped/);
    assert.match(expanded, /Output prefix: \/fixture\/jobs\/example\/output.log/);
    assert.match(expanded, /Cleanup evidence is not universal process containment/);
  } finally { syntax.dispose(); await rm(directory, { recursive: true, force: true }); }
});
