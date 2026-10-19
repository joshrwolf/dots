import assert from "node:assert/strict";
import test from "node:test";
import { stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { initTheme } from "@earendil-works/pi-coding-agent";
import { getThemeByName } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { STALL_MS, createWidget, describeActivity, describeText, projectEvent, renderEvent } from "../extensions/workers/present.ts";
import { renderWake } from "../extensions/wake/present.ts";

initTheme("dark", false);
const theme = getThemeByName("dark");
const plain = (lines) => stripTerminalSequences(lines.join("\n"));

const completed = {
  role: "custom", customType: "dots.worker.event", timestamp: 0, display: true,
  content: "Worker netpol-api [w-1a2b3c] completed · session /tmp/s.jsonl\n\nOUTCOME: policy added\n- api/deploy/networkpolicy.yaml\n- commit 0fabb23\n…\n\n[1,689 chars; full text: worker_status { id: \"w-1a2b3c\" }]",
  details: { id: "w-1a2b3c", kind: "completed", label: "netpol-api", truncated: true, elapsed: "2m", tokens: 15410, session: "/tmp/s.jsonl" },
};

test("worker event projects to a card: outcome first, pointer to the full text, recoverable when narrow", () => {
  const model = projectEvent(completed);
  assert.equal(model.heading[0].text, "worker");
  assert.equal(model.heading[1].text, "netpol-api");
  assert.equal(model.status[0].text, "completed");
  assert.match(model.body, /^OUTCOME: policy added/);
  assert.ok(model.notices.some((n) => /worker_status/.test(n.text)));

  const collapsedLines = renderEvent(completed, { expanded: false, outputPad: 1 }, theme).render(100);
  const collapsed = plain(collapsedLines);
  assert.match(stripTerminalSequences(collapsedLines[0]), /worker +netpol-api +completed/, "identity and outcome share the same header contract as tools");
  assert.match(collapsed, /│ OUTCOME: policy added/, "result preview sits under the header");
  assert.match(collapsed, /Ctrl\+O to expand/, "long results advertise expansion");
  const expanded = plain(renderEvent(completed, { expanded: true, outputPad: 1 }, theme).render(100));
  assert.match(expanded, /commit 0fabb23/);
  assert.match(expanded, /session \/tmp\/s\.jsonl/);

  for (const width of [12, 20, 24, 40]) {
    const lines = renderEvent(completed, { expanded: false, outputPad: 0 }, theme).render(width);
    assert.ok(lines.length > 0);
    for (const line of lines) assert.ok(stripTerminalSequences(line).length <= width, `width ${width}: ${stripTerminalSequences(line)}`);
    if (width >= 20) assert.match(plain(lines), /completed/, `state survives width ${width}`);
  }
  // At hopeless widths, the outcome takes priority over identity.
  assert.match(stripTerminalSequences(renderEvent(completed, { expanded: false, outputPad: 0 }, theme).render(12)[0]), /completed/);
});

test("asking and undeliverable events carry their recovery notice; control characters never render", () => {
  const asking = { ...completed, content: "Worker x [w-9] asking\n\nWhich base?\x1b[31m\x07", details: { id: "w-9", kind: "asking", label: "x\x1b[2Jbad" } };
  const out = plain(renderEvent(asking, { expanded: false, outputPad: 0 }, theme).render(80));
  assert.match(out, /answer with message \{ to: "w-9"/);
  assert.match(out, /Which base\?/);
  assert.doesNotMatch(out, /\x07|\[2J/);
  const lost = { ...completed, details: { id: "w-9", kind: "undeliverable", label: "x" } };
  assert.match(plain(renderEvent(lost, { expanded: false, outputPad: 0 }, theme).render(80)), /could not be routed/);
});

test("activity text says what a call is doing, bounded and sanitized", () => {
  assert.equal(describeActivity("bash", { command: "git status\x1b[2J" }), "bash git status");
  assert.equal(describeActivity("edit", { path: "api/x.yaml", edits: [] }), "edit api/x.yaml");
  assert.equal(describeActivity("grep", { pattern: "foo", path: "src" }), "grep foo in src");
  assert.equal(describeActivity("message", { to: "parent", text: "?" }), "message to parent");
  assert.equal(describeActivity("custom", { anything: "x".repeat(500) }).length, "custom ".length + 200);
  assert.equal(describeActivity("ls", undefined), "ls");
  assert.equal(describeText({ role: "assistant", content: [{ type: "thinking", thinking: "hm" }, { type: "text", text: "\n\nAdding the policy.\nThen tests." }] }), "Adding the policy.");
  assert.equal(describeText({ role: "user", content: [{ type: "text", text: "no" }] }), "");
});

test("widget rows keep label and state, drop tokens then calls then elapsed as width shrinks", () => {
  const rows = () => [
    { label: "netpol-api", state: "running", depth: 0, elapsed: "2m", tokens: 15410, calls: 12, detail: "bash git status", idleMs: 1000 },
    { label: "web", state: "asking", depth: 0, elapsed: "40s", tokens: 2000, calls: 3, detail: "asking: Which ingress?", idleMs: 1000 },
  ];
  const widget = createWidget(rows, theme);
  const lines = widget.render(120).map((l) => stripTerminalSequences(l));
  assert.equal(lines.length, 3);
  assert.match(lines.shift(), /^workers 2 active \/workers/);
  assert.match(lines[0], /^● netpol-api bash git status.*2m · 12 calls · 15k tok$/);
  assert.match(lines[1], /^\? web        asking: Which ingress\?.*40s · 3 calls · 2\.0k tok$/, "labels pad to one column");
  const wide = lines.join("\n");
  const narrow = plain(widget.render(30));
  assert.match(narrow, /netpol-api/);
  assert.match(narrow, /web/);
  assert.doesNotMatch(narrow, /tok|calls/, "counts are the first optional segments to go");
  assert.deepEqual(createWidget(() => [], theme).render(80), [], "nothing live, nothing drawn");
});

test("worker label columns use terminal cells, not string lengths", () => {
  const rows = ["日本語", "👩🏽‍💻", "review"].map((label) => ({ label, state: "running", depth: 0, elapsed: "1m", tokens: 1, calls: 1, detail: "read report.ts", idleMs: 0 }));
  const lines = createWidget(() => rows, theme).render(100).slice(1).map(stripTerminalSequences);
  const columns = lines.map((line) => visibleWidth(line.slice(0, line.indexOf("read report.ts"))));
  assert.ok(columns.every((column) => column === columns[0]));
});

test("widget flags a failed last call and a quiet worker", () => {
  const rows = () => [
    { label: "a", state: "running", depth: 0, elapsed: "5m", tokens: 1, calls: 4, detail: "\u201cRetrying.\u201d", failed: "bash", idleMs: 0 },
    { label: "b", state: "running", depth: 0, elapsed: "5m", tokens: 1, calls: 4, detail: "bash sleep 600", idleMs: STALL_MS + 30_000 },
    { label: "c", state: "running", depth: 0, elapsed: "5m", tokens: 1, calls: 1, detail: "worker_wait w-1", idleMs: 0 },
    { label: "leaf", state: "running", depth: 1, elapsed: "4m", tokens: 900, calls: 1, detail: "bash sleep 25", idleMs: 0 },
    { label: "twig", state: "asking", depth: 2, elapsed: "1m", tokens: 90, calls: 1, detail: "asking: ok?", idleMs: 0 },
  ];
  const lines = createWidget(rows, theme).render(100).slice(1).map((l) => stripTerminalSequences(l));
  // Column = longest (indent + label); every activity starts at the same cell.
  const col = lines.slice(0, 5).map((l) => l.search(/bash|worker_wait|asking/));
  assert.ok(col.every((c) => c === col[0] && c > 0), `aligned: ${lines.slice(0, 5).join("|")}`);
  assert.match(lines[0], /^\u2717 a +bash failed \u201cRetrying\.\u201d/);
  assert.match(lines[1], /^● b +bash sleep 600.*quiet 2m \u00b7 5m/);
  assert.match(lines[2], /^● c +worker_wait w-1.*1 call \u00b7 1 tok$/);
  assert.match(lines[3], /^\u2937 ● leaf +bash sleep 25.*900 tok$/, "child indented under its owner");
  assert.match(lines[4], /^  \u2937 \? twig +asking: ok\?/, "grandchild indented further");
  const folded = createWidget(rows, theme, 2).render(100).map((l) => stripTerminalSequences(l));
  assert.equal(folded.length, 4);
  assert.match(folded[3], /\+3 more \u00b7 \/workers/);
});

test("wake fire renders note, cadence, exit code, and lateness; failed checks preview their evidence", () => {
  const fire = {
    role: "custom", customType: "dots.wake", timestamp: 0, display: true,
    content: "Wake k-1 (fire 2 of 4, every 1m)\n\nCI changed; inspect\n\ncheck: gh run list\nexit: 1\nfailure\n\nCancel with wake_cancel { id: \"k-1\" }.",
    details: { id: "k-1", fired: 2, limit: 4, late: 5 * 60_000, every: "1m", check: "gh run list", exit: 1, text: "CI changed; inspect" },
  };
  const collapsed = plain(renderWake(fire, { expanded: false, outputPad: 0 }, theme).render(100));
  assert.match(collapsed, /wake +CI changed; inspect/);
  assert.match(collapsed, /2\/4 · every 1m/);
  assert.match(collapsed, /exit 1/);
  assert.match(collapsed, /late by 5m/);
  assert.match(collapsed, /Ctrl\+O to expand/);
  const expanded = plain(renderWake(fire, { expanded: true, outputPad: 0 }, theme).render(100));
  assert.match(expanded, /check: gh run list/);
  assert.match(expanded, /failure/);
});
