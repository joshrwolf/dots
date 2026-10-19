import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { ToolExecutionComponent, CustomMessageComponent, initTheme, type ExtensionAPI, type ToolRendererResolver } from "@earendil-works/pi-coding-agent";
import { ProcessTerminal, TuiMainScreen, Text, stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { registerUI } from "../extensions/ui/index.ts";
import { backgroundToolNames } from "../src/features/tools/activity.ts";
import { parsePreferences } from "../src/host/preferences.ts";
import { recordedJobs, renderJobMessage } from "../extensions/jobs/transcript.ts";
import { jobFixture } from "../dev/scenarios.ts";
import { summary } from "../extensions/jobs/protocol.ts";

initTheme("dark", false);
const tui = new TuiMainScreen(new ProcessTerminal());
const plain = (lines: string[]) => stripTerminalSequences(lines.join("\n"));
function registration(preferences = parsePreferences({ version: 1 })) {
  let resolver: ToolRendererResolver | undefined;
  let dispose = () => {};
  registerUI({ registerCommand() {}, registerToolRenderer(value: ToolRendererResolver) { resolver = value; },
    on(name: string, handler: () => void) { if (name === "session_shutdown") dispose = handler; },
  } as unknown as ExtensionAPI, preferences);
  assert.ok(resolver);
  const resolve = resolver;
  return { resolve, dispose, tool(name: string, args: unknown, text: string, details?: unknown, error = false) {
    const view = new ToolExecutionComponent(name, "fixture", args, { outputPad: 0 }, resolve(name, () => undefined), tui, "/workspace/example");
    view.setArgsComplete(); view.markExecutionStarted();
    view.updateResult({ content: [{ type: "text", text }], details, isError: error });
    return view;
  } };
}

test("live registration covers every owned background tool and preserves custom/native ownership", () => {
  const r = registration();
  try {
    const owned = ["jobs", "workers", "wake"].flatMap((name) => {
      const source = readFileSync(new URL(`../extensions/${name}/index.ts`, import.meta.url), "utf8");
      return [...source.matchAll(/name: "([a-z_]+)",\s*(?:\n\s*)?label:/g)].map((match) => match[1]);
    });
    assert.deepEqual([...backgroundToolNames].sort(), owned.sort());
    for (const name of [...backgroundToolNames, "grep", "find", "ls", "example_lookup"]) {
      const renderer = r.resolve(name, () => undefined);
      assert.equal(renderer?.renderShell, "self", name);
      assert.ok(renderer?.renderCall && renderer.renderResult, name);
      assert.equal(renderer, r.resolve(name, () => undefined), "stable ownership for repeated resolution");
    }
    const custom = { renderCall: () => new Text("third party", 0, 0) };
    assert.equal(r.resolve("example_lookup", () => custom), custom);
    const shellOwner = { renderShell: "self" } as const;
    assert.equal(r.resolve("example_lookup", () => shellOwner), shellOwner);
  } finally { r.dispose(); }
  const native = registration(parsePreferences({ version: 1, tools: [], otherTools: "native" }));
  try { for (const name of ["read", "job_start", "example_lookup"]) assert.equal(native.resolve(name, () => undefined), undefined); }
  finally { native.dispose(); }
  assert.equal(parsePreferences({ version: 1, tools: [] }).otherTools, "native", "older all-native preferences stay native");
  assert.throws(() => parsePreferences({ version: 1, otherTools: null }), /otherTools/);
});

test("native host job rows stay compact and expand evidence without stock shells or lost facts", () => {
  const r = registration();
  try {
    for (const phase of [0, 1, 2, 3, 4, 5]) {
      const job = jobFixture(phase);
      const original = structuredClone(job);
      const view = r.tool("job_start", { command: job.command }, summary(job), job);
      const collapsed = plain(view.render(160));
      assert.match(collapsed, /job +start npm test/);
      assert.doesNotMatch(collapsed, /job_start|command=|prefixFlushed=|Output prefix:/);
      if (phase === 0) { assert.match(collapsed, /accepted/); assert.doesNotMatch(collapsed, /exit 0/); }
      if (phase === 3) { assert.match(collapsed, /exit 0/); assert.ok(view.render(160).length < 5); }
      if (phase === 4) assert.match(collapsed, /exit 1[\s\S]*Capture incomplete/);
      if (phase === 5) assert.match(collapsed, /unknown[\s\S]*observation lost/);
      for (const expanded of [false, true]) {
        view.setExpanded(expanded);
        for (const width of [1, 2, 12, 30, 80, 160]) assert.ok(view.render(width).every((line) => visibleWidth(line) <= width), `${phase}/${expanded}/${width}`);
      }
      assert.match(plain(view.render(160)), /Output prefix: \/fixture\/jobs\/example\/output.log/);
      assert.match(plain(view.render(160)), /Cleanup evidence is not universal process containment/);
      assert.deepEqual(job, original, "presentation never rewrites snapshots");
    }
  } finally { r.dispose(); }
});

test("completion messages use recorded snapshots; cleanup and capture cannot disappear behind exit zero", () => {
  const job = jobFixture(3);
  job.supervisorExit = { code: null, signal: "SIGKILL" };
  job.facts.capture.droppedBytes = 12;
  const message = { role: "custom" as const, customType: "dots-job-result", timestamp: 0, display: true, content: summary(job), details: { id: job.id, state: job.state, job } };
  const view = new CustomMessageComponent(message, renderJobMessage, undefined, 0);
  const collapsed = plain(view.render(160));
  assert.match(collapsed, /exit 0/);
  assert.match(collapsed, /supervisor SIGKILL/);
  assert.match(collapsed, /12 bytes dropped/);
  assert.doesNotMatch(collapsed, /Output prefix:|Capture: 0 bytes/);
  view.setExpanded(true);
  assert.match(plain(view.render(160)), /Stop intent: none/);
  assert.match(plain(view.render(160)), /Output prefix:/);
  const legacy = new CustomMessageComponent({ ...message, details: { id: job.id, state: job.state } }, renderJobMessage, undefined, 0);
  assert.match(plain(legacy.render(160)), /Legacy report/);
  legacy.setExpanded(true);
  assert.match(plain(legacy.render(160)), /dropped=12/);
  assert.equal(recordedJobs({ ...job, facts: null }), undefined);
  assert.equal(recordedJobs({ jobs: Array(33).fill(job) }), undefined);
  assert.equal(recordedJobs(job)?.[0]?.state, "finished");
});

test("fallback, worker replies and failures preserve text, expansion and narrow-width safety", () => {
  const r = registration();
  try {
    const records = [
      { name: "worker_spawn", args: { label: "review" }, details: { label: "review", state: "running", processState: "alive", quiet: false }, text: "Worker review spawned" },
      { name: "worker_wait", args: { id: "w-example" }, details: { label: "review", state: "asking", processState: "alive", quiet: false, question: "Which revision?" }, text: "Which revision?\nAdditional evidence\nMore context\nFINAL DETAIL" },
      { name: "message", args: { to: "parent" }, details: { answered: false }, text: "No answer. No decision was approved." },
      { name: "wake", args: { text: "Review result", in: "15m" }, details: { text: "Review result", active: true, in: "15m" }, text: "Wake scheduled" },
      { name: "example_lookup", args: { query: "example" }, details: undefined, text: "First\nSecond\nThird\nFINAL DETAIL" },
    ];
    for (const item of records) {
      const view = r.tool(item.name, item.args, item.text, item.details);
      const collapsed = plain(view.render(120));
      if (item.name === "worker_wait") assert.match(collapsed, /Which revision/);
      if (item.name === "message") assert.match(collapsed, /No decision was approved/);
      view.setExpanded(true);
      for (const line of item.text.split("\n")) assert.ok(plain(view.render(120)).includes(line));
      for (const width of [1, 8, 20, 80]) assert.ok(view.render(width).every((line) => visibleWidth(line) <= width));
    }
    const failure = r.tool("example_lookup", {}, "Failure\nRecover from /fixture/result.log", undefined, true);
    assert.match(plain(failure.render(120)), /Recover from \/fixture\/result.log/);
    assert.match(plain(failure.render(120)), /failed/);
    const fallback = r.tool("example_lookup", { query: "example" }, "first\nsecond\nthird\nfourth");
    assert.match(plain(fallback.render(120)), /fourth/, "unknown results remain conservative, not guessed summaries");
    const many = [...Array.from({ length: 8 }, (_, i) => ({ label: `worker-${i}`, state: "completed", processState: "closed", quiet: false })), { label: "last-worker", state: "failed", processState: "closed", quiet: false, error: "Permission denied" }];
    const list = r.tool("worker_status", {}, "Full recorded inventory", many);
    assert.match(plain(list.render(120)), /last-worker: Permission denied/);
    assert.match(plain(list.render(120)), /9 workers/);
    assert.match(plain(list.render(120)), /\+1 more/);
    const quiet = r.tool("worker_status", {}, "Full recorded status", { label: "review", state: "running", processState: "alive", quiet: true, quietFor: "2m", children: { failed: 1, asking: 2 } });
    assert.match(plain(quiet.render(120)), /No recent events · 2m/);
    assert.match(plain(quiet.render(120)), /1 descendant failed/);
    assert.match(plain(quiet.render(120)), /2 descendants asking/);
    list.setExpanded(true);
    assert.match(plain(list.render(120)), /Full recorded inventory/);
  } finally { r.dispose(); }
});


test("wide transcripts keep outcomes adjacent, clip commands, and distinguish completion from invocation", () => {
  const r = registration();
  const command = "printf '日本語 👩🏽‍💻 preview'; " + "echo fixture; ".repeat(30) + "echo LAST_COMMAND_TOKEN";
  try {
    const bash = r.tool("bash", { command }, "Command exited with code 7", undefined, true);
    for (const width of [120, 240, 400]) {
      const lines = bash.render(width).map(stripTerminalSequences);
      const heading = lines.find((line) => line.includes("bash"))!;
      assert.match(heading, /exit 7/);
      assert.ok(visibleWidth(heading.slice(0, heading.indexOf("exit 7"))) < 100, "exit stays beside the bounded command");
      assert.doesNotMatch(plain(lines), /LAST_COMMAND_TOKEN|\uFFFD/);
      assert.equal(lines.filter((line) => line.includes("exit 7")).length, 1, "one collapsed diagnostic");
      assert.ok(lines.every((line) => visibleWidth(line) <= width));
    }
    bash.setExpanded(true);
    assert.match(plain(bash.render(240)), /LAST_COMMAND_TOKEN/);
    const job = jobFixture(3);
    job.command = command;
    const receipt = r.tool("job_start", { command }, summary(job), job);
    assert.match(plain(receipt.render(400)), /job +start /);
    const completion = new CustomMessageComponent({ role: "custom", customType: "dots-job-result", timestamp: 0, display: true,
      content: summary(job), details: { job },
    }, renderJobMessage, undefined, 0);
    const header = completion.render(400).map(stripTerminalSequences).find((line) => /job +finished/.test(line))!;
    assert.ok(header);
    assert.ok(visibleWidth(header.slice(0, header.indexOf("exit 0"))) < 110);
    assert.ok(header.indexOf(job.id.slice(0, 12)) > 350, "only secondary metadata uses the far edge");
    assert.doesNotMatch(header, /LAST_COMMAND_TOKEN/);
    completion.setExpanded(true);
    assert.match(plain(completion.render(240)), /LAST_COMMAND_TOKEN/);
    for (const width of [1, 8, 20, 80]) assert.ok(completion.render(width).every((line) => visibleWidth(line) <= width));
  } finally { r.dispose(); }
});
