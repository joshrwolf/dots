// Network-free compatibility tests against the installed host when available,
// otherwise the exact development SDK. No private host imports or real providers.
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, existsSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { pathToFileURL } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import test from "node:test";
import { Type } from "typebox";
import { Check } from "typebox/value";
import type { AssistantMessage, TranscriptContext, ToolCall } from "@earendil-works/pi-ai";
import type { ExtensionAPI, ExtensionContext, ExtensionFactory, ExtensionUIContext } from "@earendil-works/pi-coding-agent";
import { CompletionDelivery, registerDeliveryHooks } from "../extensions/jobs/delivery.ts";
import { ENTRY_TYPE, RECORD_TYPE, WAKE_TYPE, emptyFacts, type Job } from "../extensions/jobs/protocol.ts";
import jobsExtension from "../extensions/jobs/index.ts";
import { JobSchema, JobsSchema } from "../extensions/jobs/results.ts";

const installation = join(homedir(), ".pi", "agent", "install");
const versionFile = join(installation, "current-version");
const hostRoot = process.env.PI_JOBS_HOST_ROOT ?? (existsSync(versionFile) ? join(installation, "releases", readFileSync(versionFile, "utf8").trim(), "node_modules", "@earendil-works", "pi-coding-agent") : undefined);
const host: typeof import("@earendil-works/pi-coding-agent") = hostRoot ? await import(pathToFileURL(join(hostRoot, "dist/index.js")).href) : await import("@earendil-works/pi-coding-agent");
const ai: typeof import("@earendil-works/pi-ai") = hostRoot ? await import(pathToFileURL(join(dirname(hostRoot), "pi-ai/dist/index.js")).href) : await import("@earendil-works/pi-ai");
const label = hostRoot ? "installed host" : "development SDK";
const job = (id = "j-00000000-0000-0000-0000-000000000001"): Job => ({ version: 1, id, command: "true", cwd: "/tmp", directory: "/tmp/neutral-job", timeoutMs: 1000, startedAt: 1, finishedAt: 2, state: "finished", stopIntent: null, facts: emptyFacts(), supervisorExit: { code: null, signal: "SIGKILL" }, observationLoss: null, artifactsEvicted: false });
const text = (): AssistantMessage["content"] => [{ type: "text", text: "fixture response" }];
const call = (name: string, args: ToolCall["arguments"]): AssistantMessage["content"] => [{ type: "toolCall", id: "fixture-call", name, arguments: args }];
async function until(predicate: () => boolean): Promise<void> {
  const end = Date.now() + 10_000;
  while (!predicate()) { if (Date.now() > end) throw new Error("host fixture deadline"); await delay(10); }
}
function gate() { let release!: () => void; return { promise: new Promise<void>((r) => { release = r; }), release: () => release() }; }

type Script = (n: number, context: TranscriptContext, signal: AbortSignal | undefined) => Promise<AssistantMessage["content"]>;
async function setup(script: Script, real = false, codemode = false) {
  const dir = mkdtempSync(join(tmpdir(), "dots-jobs-host-"));
  const previous = process.env.PI_CODING_AGENT_DIR;
  process.env.PI_CODING_AGENT_DIR = dir;
  const requests: TranscriptContext[] = [];
  const errors: unknown[] = [];
  const toolResults: { toolName: string; content: unknown; structuredContent?: unknown; details: unknown; isError: boolean }[] = [];
  let context: ExtensionContext | undefined;
  let delivery: CompletionDelivery | undefined;
  let api: ExtensionAPI | undefined;
  const toolGate = gate();
  const extension: ExtensionFactory = async (pi) => {
    api = pi;
    if (codemode) await host.createCodemodeExtension({ models: false })(pi);
    pi.on("tool_result", (event) => { toolResults.push(structuredClone(event)); });
    if (real) jobsExtension(pi);
    else {
      delivery = new CompletionDelivery(pi, () => context);
      registerDeliveryHooks(pi, () => delivery);
      pi.on("session_shutdown", () => delivery?.close());
    }
    pi.on("session_start", (_event, ctx) => { context = ctx; });
    pi.registerTool({ name: "fixture_wait", label: "Fixture wait", description: "Wait for a test gate", parameters: Type.Object({}), async execute() {
      await toolGate.promise;
      return { content: [{ type: "text", text: "fixture tool complete" }], details: undefined };
    } });
  };
  const settings = host.SettingsManager.inMemory({ compaction: { enabled: false }, retry: { enabled: false }, cacheWarming: "off" });
  const loader = new host.DefaultResourceLoader({ cwd: dir, agentDir: dir, settingsManager: settings, noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, extensionFactories: [extension] });
  await loader.reload();
  const runtime = await host.ModelRuntime.create({ authPath: join(dir, "auth.json"), modelsPath: null, modelsStorePath: join(dir, "models-store.json"), allowModelNetwork: false, refreshOnCreate: false });
  runtime.registerProvider("jobs-fixture", {
    api: "jobs-fixture", apiKey: "not-a-real-key", baseUrl: "https://example.invalid",
    models: [{ id: "fixture", name: "Fixture", reasoning: false, input: ["text"], contextWindow: 100_000, maxTokens: 1000, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 } }],
    streamSimple(model, ctx, options) {
      const stream = ai.createAssistantMessageEventStream();
      const n = requests.push(structuredClone(ctx));
      const message: AssistantMessage = { role: "assistant", content: [], api: model.api, provider: model.provider, model: model.id, timestamp: Date.now(), stopReason: "pending", usage: { input: 1, output: 1, cacheRead: 0, cacheWrite: 0, totalTokens: 2, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } } };
      void (async () => {
        try {
          await options?.onPayload?.({ fixture: true }, model);
          await options?.onResponse?.({ status: 200, headers: {} }, model);
          stream.push({ type: "start", partial: message });
          const abort = new Promise<never>((_resolve, reject) => {
            if (options?.signal?.aborted) reject(new Error("aborted"));
            else options?.signal?.addEventListener("abort", () => reject(new Error("aborted")), { once: true });
          });
          message.content = await Promise.race([script(n, ctx, options?.signal), abort]);
          for (const [contentIndex, block] of message.content.entries()) {
            if (block.type === "text") {
              stream.push({ type: "text_start", contentIndex, partial: message });
              stream.push({ type: "text_delta", contentIndex, delta: block.text, partial: message });
              stream.push({ type: "text_end", contentIndex, content: block.text, partial: message });
            } else if (block.type === "toolCall") {
              stream.push({ type: "toolcall_start", contentIndex, partial: message });
              stream.push({ type: "toolcall_delta", contentIndex, delta: JSON.stringify(block.arguments), partial: message });
              stream.push({ type: "toolcall_end", contentIndex, toolCall: block, partial: message });
            }
          }
          message.stopReason = message.content.some((c) => c.type === "toolCall") ? "toolUse" : "stop";
          stream.push({ type: "done", reason: message.stopReason, message });
        } catch (error) {
          message.stopReason = options?.signal?.aborted ? "aborted" : "error";
          message.errorMessage = String(error);
          stream.push({ type: "error", reason: message.stopReason, error: message });
        } finally { stream.end(); }
      })();
      return stream;
    },
  });
  const model = runtime.getModel("jobs-fixture", "fixture");
  assert.ok(model);
  const { session } = await host.createAgentSession({ cwd: dir, agentDir: dir, modelRuntime: runtime, model, settingsManager: settings, sessionManager: host.SessionManager.inMemory(dir), resourceLoader: loader, noTools: "builtin" });
  await session.bindExtensions({ mode: "rpc", onError: (e) => errors.push(e) });
  if (codemode) session.setActiveToolsByName([...session.getActiveToolNames(), "codemode"]);
  return {
    session, requests, toolGate, dir, toolResults,
    complete: (j = job()) => { assert.ok(api); assert.ok(delivery); api.appendEntry(ENTRY_TYPE, j); delivery.record(j); },
    user: () => { assert.ok(api); api.sendUserMessage("new fixture user input"); },
    async close() {
      toolGate.release();
      await session.abort();
      await session.extensionRunner.emit({ type: "session_shutdown", reason: "quit" });
      session.dispose();
      assert.deepEqual(errors, []);
      if (previous === undefined) delete process.env.PI_CODING_AGENT_DIR; else process.env.PI_CODING_AGENT_DIR = previous;
      rmSync(dir, { recursive: true, force: true });
    },
  };
}
const records = (h: Awaited<ReturnType<typeof setup>>) => h.session.sessionManager.getBranch().filter((e) => e.type === "custom_message" && e.customType === RECORD_TYPE);
const hasResult = (ctx: TranscriptContext | undefined) => JSON.stringify(ctx?.messages).includes("root outcome unobserved");

test(`${label}: idle record is durable before one coalesced separate wake`, { timeout: 15_000 }, async () => {
  const h = await setup(async () => text());
  try {
    h.complete();
    h.complete(job("j-00000000-0000-0000-0000-000000000002"));
    assert.equal(records(h).length, 2);
    await until(() => h.requests.length === 1 && !h.session.isStreaming);
    assert.equal(hasResult(h.requests[0]), true);
    const entries = h.session.sessionManager.getBranch();
    assert.equal(entries.filter((e) => e.type === "custom_message" && e.customType === WAKE_TYPE).length, 1);
    assert.ok(entries.findIndex((e) => e.type === "custom" && e.customType === ENTRY_TYPE) < entries.findIndex((e) => e.type === "custom_message" && e.customType === RECORD_TYPE));
  } finally { await h.close(); }
});

test(`${label}: final streaming completion needs exactly one continuation`, { timeout: 15_000 }, async () => {
  const streaming = gate();
  const h = await setup(async (n) => { if (n === 1) await streaming.promise; return text(); });
  try {
    const prompt = h.session.prompt("fixture user input");
    await until(() => h.requests.length === 1);
    h.complete();
    h.session.clearQueue(); // TUI Escape's queue-clear operation must not drop the record.
    streaming.release();
    await prompt;
    assert.equal(h.requests.length, 2);
    assert.equal(hasResult(h.requests[1]), true);
    assert.equal(records(h).length, 1);
  } finally { streaming.release(); await h.close(); }
});

test(`${label}: completion during a tool is visible next turn without redundant continuation`, { timeout: 15_000 }, async () => {
  const h = await setup(async (n) => n === 1 ? call("fixture_wait", {}) : text());
  let toolStarted = false;
  h.session.subscribe((e) => { if (e.type === "tool_execution_start") toolStarted = true; });
  try {
    const prompt = h.session.prompt("fixture user input");
    await until(() => toolStarted);
    h.complete();
    h.toolGate.release();
    await prompt;
    assert.equal(h.requests.length, 2);
    assert.equal(hasResult(h.requests[1]), true);
    assert.equal(records(h).length, 1);
  } finally { await h.close(); }
});

test(`${label}: abort retains busy record, suppresses idle completions until sendUserMessage`, { timeout: 15_000 }, async () => {
  const streaming = gate();
  const h = await setup(async (n) => { if (n === 1) await streaming.promise; return text(); });
  const settled: boolean[] = [];
  h.session.subscribe((e) => { if (e.type === "agent_settled") settled.push(e.aborted); });
  try {
    const prompt = h.session.prompt("fixture user input");
    await until(() => h.requests.length === 1);
    h.complete();
    h.session.clearQueue();
    await h.session.abort();
    await prompt;
    assert.equal(h.requests.length, 1);
    assert.equal(records(h).length, 1);
    assert.equal(settled.at(-1), true);
    h.complete(job("j-00000000-0000-0000-0000-000000000002"));
    await delay(50);
    assert.equal(h.requests.length, 1);
    assert.equal(records(h).length, 2);
    h.user();
    await until(() => h.requests.length === 2 && !h.session.isStreaming);
    h.complete(job("j-00000000-0000-0000-0000-000000000003"));
    await until(() => h.requests.length === 3 && !h.session.isStreaming);
  } finally { streaming.release(); await h.close(); }
});

for (const reason of ["quit", "reload", "new", "resume", "fork"] as const) test(`${label}: direct ${reason} lifecycle hook stops jobs without model messages; live tree navigation is refused`, { skip: process.platform !== "darwin", timeout: 12_000 }, async () => {
  const h = await setup(async () => text(), true);
  try {
    const runner = h.session.extensionRunner;
    const start = runner.getToolDefinition("job_start");
    assert.ok(start);
    await start.execute("fixture-start", { command: "sleep 20", timeoutMs: 10_000 }, undefined, undefined, runner.createToolContext("fixture-start", undefined));
    const tree = await runner.emit({ type: "session_before_tree", preparation: { targetId: "fixture", oldLeafId: null, commonAncestorId: null, entriesToSummarize: [], userWantsSummary: false }, signal: AbortSignal.timeout(5000) });
    assert.equal(tree?.cancel, true);
    await runner.emit({ type: "session_shutdown", reason });
    assert.equal(records(h).length, 0);
    assert.equal(h.requests.length, 0);
    const final = h.session.sessionManager.getBranch().filter((e) => e.type === "custom" && e.customType === ENTRY_TYPE).at(-1);
    assert.ok(final && final.type === "custom");
    assert.equal((final.data as Job).stopIntent, "owner-shutdown");
    assert.ok(["finished", "unknown"].includes((final.data as Job).state));
    await assert.rejects(() => start.execute("after-shutdown", { command: "true" }, undefined, undefined, runner.createToolContext("after-shutdown", undefined)), /not active/);
  } finally { await h.close(); }
});

test(`${label}: direct one-shot mode admission is rejected`, { timeout: 10_000 }, async () => {
  const h = await setup(async () => text(), true);
  try {
    const runner = h.session.extensionRunner;
    const start = runner.getToolDefinition("job_start");
    assert.ok(start);
    for (const mode of ["print", "json"] as const) {
      runner.setUIContext(undefined, mode);
      await assert.rejects(() => start.execute("fixture", { command: "true" }, undefined, undefined, runner.createToolContext("fixture", undefined)), /one-shot modes/);
    }
    assert.equal(h.session.sessionManager.getBranch().filter((e) => e.type === "custom" && e.customType === ENTRY_TYPE).length, 0);
  } finally { await h.close(); }
});

test(`${label}: human picker stop records completion without a model turn and tree/shutdown release its widget`, { skip: process.platform !== "darwin", timeout: 15_000 }, async () => {
  const h = await setup(async () => text(), true);
  try {
    const runner = h.session.extensionRunner;
    const widgetChanges: boolean[] = [];
    let cancel = true;
    const ui: ExtensionUIContext = {
      ...runner.getUIContext(),
      setWidget(_key, content) { widgetChanges.push(content !== undefined); },
      select: async (_title, choices) => { assert.ok(choices.includes("Stop job")); return cancel ? undefined : "Stop job"; },
      confirm: async () => true,
    };
    runner.setUIContext(ui, "tui");
    await runner.emit({ type: "session_start", reason: "startup" });
    const start = runner.getToolDefinition("job_start");
    const command = runner.getCommand("jobs");
    assert.ok(start && command);
    const receipt = await start.execute("ui-fixture", { command: "sleep 20", timeoutMs: 10000 }, undefined, undefined, runner.createToolContext("ui-fixture", undefined));
    const receiptData = receipt.structuredContent;
    assert.ok(Check(JobSchema, receiptData));
    const id = receiptData.id;
    assert.equal(widgetChanges.at(-1), true);
    const before = h.session.sessionManager.getBranch().length;
    await command.handler(id, runner.createCommandContext());
    assert.equal(h.session.sessionManager.getBranch().length, before); // Cancel writes nothing.
    cancel = false;
    await command.handler(id, runner.createCommandContext());
    await until(() => records(h).length === 1);
    await delay(100);
    assert.equal(h.requests.length, 0);
    assert.equal(h.session.sessionManager.getBranch().filter((e) => e.type === "custom_message" && e.customType === WAKE_TYPE).length, 0);
    await runner.emit({ type: "session_tree", newLeafId: null, oldLeafId: null });
    assert.equal(widgetChanges.at(-1), false);
    const changes = widgetChanges.length;
    await runner.emit({ type: "session_shutdown", reason: "reload" });
    await delay(1100);
    assert.equal(widgetChanges.length, changes);
  } finally { await h.close(); }
});

test(`${label}: RPC/headless /jobs inspection does not prompt or start a model turn`, { timeout: 10_000 }, async () => {
  const h = await setup(async () => text(), true);
  try {
    const runner = h.session.extensionRunner;
    const command = runner.getCommand("jobs");
    assert.ok(command);
    for (const mode of ["rpc", "print", "json", "tui"] as const) {
      runner.setUIContext(undefined, mode);
      await command.handler("", runner.createCommandContext());
    }
    assert.equal(h.requests.length, 0);
    assert.equal(h.session.sessionManager.getBranch().filter((e) => e.type === "custom_message" && e.customType === "dots-jobs-inspection").length, 4);
  } finally { await h.close(); }
});

test(`${label}: native codemode receives bounded structured receipts and full status/stop snapshots`, { skip: process.platform !== "darwin", timeout: 15_000 }, async () => {
  const code = `// @options: {"timeout_ms": 10000}
const receipt = await tools.job_start({ command: "sleep 20", timeoutMs: 10000 });
if (typeof receipt !== "object" || receipt.state !== "accepted" || !receipt.id || !receipt.directory) throw new Error("structured receipt missing");
const status = await tools.job_status({ id: receipt.id });
const list = await tools.job_status({});
const stopped = await tools.job_stop({ id: receipt.id });
const repeated = await tools.job_stop({ id: receipt.id });
const after = await tools.job_status({ id: receipt.id });
if (list.jobs.length !== 1 || status.jobs[0].id !== receipt.id || after.jobs[0].stopIntent !== "stop") throw new Error("structured status missing");
if (stopped.state !== "stopping" || repeated.state !== "stopping") throw new Error("stop snapshots missing");
for (const snapshot of [receipt, ...status.jobs, ...list.jobs, stopped, repeated, ...after.jobs]) {
  if (!snapshot.facts || !snapshot.facts.capture || typeof snapshot.facts.capture.bytesSeen !== "number" || typeof snapshot.facts.termSent !== "boolean" || !("observationLoss" in snapshot) || !("supervisorExit" in snapshot)) throw new Error("incomplete facts");
}
text("JOBS_STRUCTURED_OK");`;
  const h = await setup(async (n) => n === 1 ? call("codemode", { code }) : text(), true, true);
  try {
    await h.session.prompt("Run the fixture codemode contract.");
    const runner = h.session.extensionRunner;
    const result = h.toolResults.find((r) => r.toolName === "codemode");
    assert.ok(result);
    assert.equal(result.isError, false, JSON.stringify(result.content));
    assert.match(JSON.stringify(result.content), /JOBS_STRUCTURED_OK/);
    const results = h.toolResults.filter((r) => r.toolName.startsWith("job_"));
    assert.equal(results.length, 6);
    for (const r of results) {
      assert.equal(r.isError, false);
      const schema = runner.getToolDefinition(r.toolName)?.outputSchema;
      assert.ok(schema, `${r.toolName} must advertise structured output`);
      assert.equal(Check(schema, r.structuredContent), true, `${r.toolName} output must match its schema`);
      assert.deepEqual(r.details, r.structuredContent);
    }
    assert.equal(Check(JobSchema, { ...job(), command: "x".repeat(8193) }), false);
    assert.equal(Check(JobsSchema, { jobs: Array.from({ length: 33 }, () => job()) }), false);
    assert.equal(h.requests.length, 2); // Real VM/tools, scripted network-free provider.
  } finally { await h.close(); }
});

test(`${label}: actual registered job_start returns before completion, then wakes`, { skip: process.platform !== "darwin", timeout: 20_000 }, async () => {
  const h = await setup(async (n) => n === 1 ? call("job_start", { command: "printf 'neutral fixture output\\n'", timeoutMs: 5000 }) : text(), true);
  try {
    await h.session.prompt("fixture job request");
    assert.equal(h.requests.length, 2);
    assert.equal(records(h).length, 0);
    await until(() => records(h).length === 1 && h.requests.length === 3 && !h.session.isStreaming);
    const entry = h.session.sessionManager.getBranch().filter((e) => e.type === "custom" && e.customType === ENTRY_TYPE).at(-1);
    assert.ok(entry && entry.type === "custom");
    const result = entry.data as Job;
    assert.equal(result.state, "finished");
    assert.equal(result.facts.root?.code, 0);
    assert.match(readFileSync(join(result.directory, "output.log"), "utf8"), /neutral fixture output/);
    await h.session.prompt("/jobs");
    assert.equal(h.requests.length, 3);
  } finally { await h.close(); }
});
