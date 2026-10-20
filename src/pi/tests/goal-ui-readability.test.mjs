import assert from "node:assert/strict";
import test from "node:test";
import { ToolExecutionComponent, CustomMessageComponent, initTheme } from "@earendil-works/pi-coding-agent";
import { stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { registerUI } from "../extensions/ui/index.ts";
import { parsePreferences } from "../src/host/preferences.ts";
import { renderEvent } from "../extensions/workers/present.ts";
import { MarkdownView } from "../src/ui/markdown.ts";
import { getThemeByName } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

initTheme("dark", false);
const plain = (view, width = 160) => stripTerminalSequences(view.render(width).join("\n"));
function setup() {
  let resolver, dispose;
  registerUI({ registerCommand() {}, registerMarkdownTransformer() {}, registerToolRenderer(value) { resolver = value; }, on(name, handler) { if (name === "session_shutdown") dispose = handler; } }, parsePreferences({ version: 1 }));
  return { dispose, tool(name, text, details) {
    const view = new ToolExecutionComponent(name, "fixture", {}, { outputPad: 0 }, resolver(name, () => undefined), { requestRender() {} }, "/fixture");
    view.setArgsComplete(); view.markExecutionStarted();
    view.updateResult({ content: [{ type: "text", text }], details, isError: false });
    return view;
  } };
}

test("worker completions render Markdown; inspections fold repeat reports without hiding warnings", () => {
  const event = new CustomMessageComponent({ role: "custom", customType: "worker", timestamp: 0, display: true,
    content: "Worker completed\n\n**Verified.** Use one interface.\n\nSecond paragraph.", details: { id: "w-fixture", label: "reference", kind: "completed" } }, renderEvent, undefined, 0);
  assert.match(plain(event), /Verified/);
  assert.doesNotMatch(plain(event), /\*\*Verified/);
  const r = setup();
  try {
    const view = r.tool("worker_status", "Verified result\nTranscript evidence", { label: "reference", state: "completed", processState: "alive", quiet: false, result: "Verified result", lastFailure: "previous check" });
    assert.match(plain(view), /completed/);
    assert.match(plain(view), /Result available/);
    assert.match(plain(view), /previous check/);
    assert.doesNotMatch(plain(view), /Verified result|process alive/);
    view.setExpanded(true);
    assert.match(plain(view), /Verified result/);
    assert.match(plain(view), /Transcript evidence/);
    assert.match(plain(view), /process alive/);
  } finally { r.dispose(); }
});

test("codemode labels paths from recorded prefixes; only plain successful write receipts fold", () => {
  const r = setup();
  try {
    const args = '{"path":"/fixture/project/wiki/design.md","content":"abbreviated';
    const details = { calls: [{ name: "write", args, status: "ok" }] };
    const original = structuredClone(details);
    const view = r.tool("codemode", "==> text 1/1 <==\nSuccessfully wrote to /fixture/project/wiki/design.md", details);
    assert.match(plain(view), /wiki\/design.md/);
    assert.match(plain(view), /1 done/);
    assert.doesNotMatch(plain(view), /content|Successfully wrote|\/fixture\/project/);
    view.setExpanded(true);
    assert.match(plain(view), /content/);
    assert.match(plain(view), /Successfully wrote/);
    assert.deepEqual(details, original);
    const diagnostic = r.tool("codemode", "WARNING: unexpected filesystem condition", details);
    assert.match(plain(diagnostic), /WARNING/);
    const unknown = r.tool("codemode", "Important unknown output", { calls: [{ name: "custom", args: "opaque preview", status: "ok" }] });
    assert.match(plain(unknown), /opaque preview/);
    const malformed = r.tool("codemode", "output", { calls: [{ name: "write", args: '{"path":"unfinished', status: "error", error: "Permission denied" }] });
    assert.match(plain(malformed), /Permission denied/);
    for (const component of [view, diagnostic, unknown, malformed]) {
      for (const expanded of [false, true]) {
        component.setExpanded(expanded);
        for (const width of [1, 2, 20, 45, 120]) assert.ok(component.render(width).every((line) => visibleWidth(line) <= width));
      }
    }
  } finally { r.dispose(); }
});

test("Markdown prose is bounded, sanitized and safe at narrow widths", () => {
  for (const name of ["dark", "light"]) {
    const view = new MarkdownView("# 日本語 👩‍💻\n\n**Result**\n\x1b]52;c;payload\x07\n" + "fixture\n".repeat(500), getThemeByName(name));
    for (const width of [1, 2, 20, 80]) {
      assert.ok(view.render(width).every((line) => visibleWidth(line) <= width));
      assert.doesNotMatch(plain(view, width), /\x1b|payload/);
    }
    assert.match(plain(view), /display limited/);
  }
});
