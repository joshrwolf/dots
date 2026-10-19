// Public SDK compatibility with the installed host, without credentials/network.
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, existsSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import goal, { FOCUS_ENTRY } from "../extensions/goal/index.ts";
import { source } from "./goal-fixture.mjs";
const installation = join(homedir(), ".pi/agent/install");
const version = join(installation, "current-version");
const root = existsSync(version) ? join(installation, "releases", readFileSync(version, "utf8").trim(), "node_modules/@earendil-works/pi-coding-agent") : undefined;
const host = root ? await import(pathToFileURL(join(root, "dist/index.js")).href) : await import("@earendil-works/pi-coding-agent");
const ai = root ? await import(pathToFileURL(join(dirname(root), "pi-ai/dist/index.js")).href) : await import("@earendil-works/pi-ai");

function promptSections(context) {
  const sections = {};
  for (const message of context.messages) {
    if (message.role !== "system") continue;
    for (const [name, value] of Object.entries(message.sections ?? {})) {
      if (value === null) delete sections[name]; else sections[name] = value;
    }
  }
  return Object.values(sections).join("\n");
}

test("real host restores goal focus and fresh context from disk without autonomous turns", { timeout: 20_000 }, async () => {
  const dir = mkdtempSync(join(tmpdir(), "dots-goal-host-"));
  const previous = process.env.PI_CODING_AGENT_DIR;
  process.env.PI_CODING_AGENT_DIR = dir;
  const backend = source(), requests = [], errors = [];
  let focusNext = false;
  const runtime = await host.ModelRuntime.create({ authPath: join(dir, "auth.json"), modelsPath: null, modelsStorePath: join(dir, "models.json"), allowModelNetwork: false, refreshOnCreate: false });
  runtime.registerProvider("goal-fixture", {
    api: "goal-fixture", apiKey: "fixture", baseUrl: "https://example.invalid",
    models: [{ id: "fixture", name: "Fixture", reasoning: false, input: ["text"], contextWindow: 100_000, maxTokens: 1000, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 } }],
    streamSimple(model, context) {
      requests.push(structuredClone(context));
      const stream = ai.createAssistantMessageEventStream();
      const message = { role: "assistant", content: [{ type: "text", text: "Fixture response" }], api: model.api, provider: model.provider, model: model.id, timestamp: Date.now(), stopReason: "stop",
        usage: { input: 1, output: 1, cacheRead: 0, cacheWrite: 0, totalTokens: 2, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } } };
      if (focusNext) {
        focusNext = false;
        message.content = [{ type: "toolCall", id: "focus-fixture", name: "goal_focus", arguments: { id: "t-123abc" } }];
        message.stopReason = "toolUse";
      }
      queueMicrotask(() => { stream.push({ type: "start", partial: message }); stream.push({ type: "done", reason: message.stopReason, message }); stream.end(); });
      return stream;
    },
  });
  async function create(manager) {
    const settings = host.SettingsManager.inMemory({ compaction: { enabled: false }, retry: { enabled: false }, cacheWarming: "off" });
    const loader = new host.DefaultResourceLoader({ cwd: dir, agentDir: dir, settingsManager: settings, noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, extensionFactories: [(pi) => goal(pi, backend)] });
    await loader.reload();
    const { session } = await host.createAgentSession({ cwd: dir, agentDir: dir, settingsManager: settings, resourceLoader: loader, modelRuntime: runtime, model: runtime.getModel("goal-fixture", "fixture"), sessionManager: manager, noTools: "builtin" });
    await session.bindExtensions({ mode: "rpc", onError: (error) => errors.push(error) });
    return session;
  }
  async function close(session) {
    await session.abort();
    await session.extensionRunner.emit({ type: "session_shutdown", reason: "quit" });
    session.dispose();
  }
  let session;
  try {
    session = await create(host.SessionManager.create(dir, join(dir, "sessions")));
    await session.prompt("/goal focus t-123abc");
    assert.equal(requests.length, 0);
    const saved = session.sessionManager.getBranch().find((e) => e.type === "custom" && e.customType === FOCUS_ENTRY);
    assert.equal(saved.data.focus.id, "t-123abc");
    await session.prompt("Discuss the design; do not execute work.");
    assert.equal(requests.length, 1);
    assert.match(promptSections(requests.at(-1)), /Use one shared export interface/);
    const file = session.sessionFile;
    assert.ok(file && existsSync(file));
    await close(session); session = undefined;
    backend.brief = async () => ({ text: "Fresh accepted goal pattern after restart" });
    session = await create(host.SessionManager.open(file));
    assert.equal(requests.length, 1, "restore must not prompt");
    await session.prompt("/goal show");
    assert.equal(requests.length, 1, "inspection must not prompt");
    await session.prompt("Discuss the revised brief.");
    assert.equal(requests.length, 2);
    assert.match(promptSections(requests.at(-1)), /Fresh accepted goal pattern after restart/);
    assert.doesNotMatch(promptSections(requests.at(-1)), /Use one shared export interface/);
    await session.prompt("/goal unfocus");
    await session.prompt("Unrelated discussion.");
    assert.equal(requests.length, 3);
    assert.doesNotMatch(promptSections(requests.at(-1)), /Focused goal context|Fresh accepted goal pattern after restart/);
    focusNext = true;
    await session.prompt("/goal new Consistent fixture exports");
    await session.waitForIdle();
    assert.equal(requests.length, 5, "new starts planning; tool use gets one normal follow-up");
    assert.match(JSON.stringify(requests.at(-2)), /Consistent fixture exports/);
    const latest = session.sessionManager.getBranch().filter((e) => e.type === "custom" && e.customType === FOCUS_ENTRY).at(-1);
    assert.equal(latest.data.focus.id, "t-123abc");
    assert.ok(session.messages.some((m) => m.role === "toolResult" && m.toolName === "goal_focus" && !m.isError));
    assert.deepEqual(errors, []);
  } finally {
    if (session) await close(session);
    if (previous === undefined) delete process.env.PI_CODING_AGENT_DIR; else process.env.PI_CODING_AGENT_DIR = previous;
    rmSync(dir, { recursive: true, force: true });
  }
});
