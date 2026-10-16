import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import register from "../extensions/workers/index.ts";

// The extension's lifecycle against a scripted `pi`: spawn -> event -> wake,
// message in every direction, late settles as updates, wait, reload -> paused
// -> continue, stop. No model, no credentials.
const FAKE_PI = `#!/usr/bin/env node
const args = process.argv.slice(2);
if (args.includes("--model") && args[args.indexOf("--model") + 1] === "broken/model") { process.stderr.write("Model not found\\n"); process.exit(2); }
require("fs").writeFileSync(require("path").join(require("os").tmpdir(), "dots-fake-pi-args-" + (process.env.DOTS_WORKER_ID || "root")), JSON.stringify(args));
const sessionFile = (args.includes("--session") ? args[args.indexOf("--session") + 1] : args[args.indexOf("--session-dir") + 1] + "/session.jsonl");
const out = (o) => process.stdout.write(JSON.stringify(o) + "\\n");
const settle = (text) => { global.last = text; out({ type: "agent_settled", aborted: false }); };
let buffer = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  buffer += chunk;
  let i;
  while ((i = buffer.indexOf("\\n")) !== -1) {
    const cmd = JSON.parse(buffer.slice(0, i)); buffer = buffer.slice(i + 1);
    const ok = (data) => out({ id: cmd.id, type: "response", command: cmd.type, success: true, data });
    if (cmd.type === "prompt") {
      ok({ disposition: "started" });
      out({ type: "tool_execution_start", toolCallId: "t", toolName: "bash", args: {} });
      out({ type: "message_update", usage: { totalTokens: 7 }, assistantMessageEvent: { type: "text_delta", delta: "x" } });
      const m = cmd.message;
      if (m.startsWith("ask:")) { out({ type: "extension_ui_request", id: "ui-9", method: "input", title: m.slice(4) }); return; }
      if (m.startsWith("confirm:")) { out({ type: "extension_ui_request", id: "ui-c", method: "confirm", title: m.slice(8) }); return; }
      if (m.startsWith("slow:")) { setTimeout(() => settle("slow done"), 400); return; }
      if (m.startsWith("long:")) { settle("OUTCOME first line\\n" + Array.from({ length: 200 }, (_, i) => "detail line " + i).join("\\n")); return; }
      if (m.startsWith("spontaneous:")) { settle("first answer"); setTimeout(() => settle("later chatter"), 120); return; }
      if (m.startsWith("tell-parent:")) { out({ type: "extension_ui_request", id: "n1", method: "notify", message: "dots.message:" + JSON.stringify({ to: "parent", text: m.slice(12) }) }); settle("told"); return; }
      if (m.startsWith("tell-sibling:")) { const [to, ...rest] = m.slice(13).split(" "); out({ type: "extension_ui_request", id: "n2", method: "notify", message: "dots.message:" + JSON.stringify({ to, text: rest.join(" ") }) }); settle("relayed"); return; }
      settle("done: " + m);
    } else if (cmd.type === "extension_ui_response") {
      settle(cmd.id === "ui-c" ? "confirmed=" + cmd.confirmed : "answered with " + cmd.value);
    } else if (cmd.type === "steer") {
      ok({ disposition: "queued" }); setTimeout(() => settle("steered: " + cmd.message), 50);
    } else if (cmd.type === "abort") {
      ok({}); out({ type: "agent_settled", aborted: true });
    } else if (cmd.type === "get_last_assistant_text") {
      ok({ text: global.last ?? null });
    } else if (cmd.type === "get_state") {
      ok({ sessionFile });
    } else { ok({}); }
  }
});
process.stdin.on("end", () => process.exit(0));
`;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(predicate, ms = 3000) {
  const deadline = Date.now() + ms;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error("timed out waiting");
    await sleep(20);
  }
}

function harness(root) {
  const handlers = {};
  const tools = new Map();
  const messages = [];
  const entries = [];
  const pi = {
    on: (name, fn) => { handlers[name] = fn; },
    registerTool: (tool) => tools.set(tool.name, tool),
    registerCommand: () => {},
    sendMessage: (message, options) => messages.push({ message, options }),
    appendEntry: (customType, data) => entries.push({ type: "custom", customType, data }),
  };
  register(pi);
  const ctx = { cwd: root, hasUI: false, sessionManager: { getBranch: () => entries } };
  const run = (name, params) => tools.get(name).execute("call", params, undefined, undefined, ctx);
  const call = async (name, params) => (await run(name, params)).details;
  return { handlers, tools, messages, entries, ctx, call, run };
}

test("worker lifecycle against a scripted pi", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "dots-workers-life-"));
  const fake = path.join(root, "fake-pi.js");
  writeFileSync(fake, FAKE_PI);
  chmodSync(fake, 0o755);
  const saved = { ...process.env };
  process.env.DOTS_PI_BIN = fake;
  process.env.PI_CODING_AGENT_DIR = path.join(root, "agent");
  delete process.env.DOTS_WORKER_ID;
  const harnesses = [];
  try {
    const h = harness(root);
    harnesses.push(h);
    h.handlers.session_start({ reason: "startup" }, h.ctx);
    assert.deepEqual([...h.tools.keys()].sort(), ["message", "worker_spawn", "worker_status", "worker_stop", "worker_wait"]);

    // spawn -> completion event wakes the owner with the child's final text
    const spawned = await h.call("worker_spawn", { label: "one", task: "count things" });
    assert.equal(spawned.state, "running");
    await until(() => h.messages.length === 1);
    assert.equal(h.messages[0].message.details.kind, "completed");
    assert.match(h.messages[0].message.content, /done: count things/);
    assert.deepEqual(h.messages[0].options, { triggerTurn: true, deliverAs: "steer" });
    assert.ok((await h.call("worker_status", { id: spawned.id }))[0].session.endsWith("session.jsonl"));

    // child asks -> asking event -> message answers it -> completion
    const asker = await h.call("worker_spawn", { label: "asker", task: "ask:Which base?" });
    await until(() => h.messages.length === 2);
    assert.equal(h.messages[1].message.details.kind, "asking");
    assert.match(h.messages[1].message.content, /Which base\?/);
    const answered = await h.run("message", { to: asker.id, text: "main" });
    assert.match(answered.content[0].text, /answered its question/);
    await until(() => h.messages.length === 3);
    assert.match(h.messages[2].message.content, /answered with main/);

    // child -> parent (non-blocking) and child -> sibling (relayed by the owner)
    await h.call("worker_spawn", { label: "teller", task: "tell-parent:heads up" });
    await until(() => h.messages.length === 5);
    assert.deepEqual(h.messages.slice(3).map((m) => m.message.details.kind).sort(), ["completed", "message"]);
    assert.ok(h.messages.slice(3).some((m) => /heads up/.test(m.message.content)));
    const before = h.messages.length;
    await h.call("worker_spawn", { label: "gossip", task: `tell-sibling:${spawned.id} ping` });
    await until(() => h.messages.length >= before + 3);
    const relay = h.messages.find((m) => m.message.details.kind === "relay");
    assert.equal(relay.message.details.to, spawned.id);
    assert.deepEqual(relay.options, { triggerTurn: false });
    // the target had finished, so the relayed text continued it in its own session
    await until(() => h.messages.some((m) => /done: Message from worker gossip/.test(m.message.content)));

    // a long result reaches the owner bounded; the full text stays on the worker
    const verbose = await h.call("worker_spawn", { label: "verbose", task: "long:go" });
    await until(() => h.messages.some((m) => m.message.details.id === verbose.id));
    const event = h.messages.find((m) => m.message.details.id === verbose.id).message;
    assert.match(event.content, /OUTCOME first line/);
    assert.doesNotMatch(event.content, /detail line 150/);
    assert.match(event.content, /full text: worker_status/);
    assert.equal(event.details.truncated, true);
    assert.ok(event.content.length < 2000);
    assert.match((await h.call("worker_status", { id: verbose.id }))[0].result, /detail line 199/);

    // a settle we did not ask for is an update and keeps the real result
    const chatty = await h.call("worker_spawn", { label: "chatty", task: "spontaneous:go" });
    await until(() => h.messages.filter((m) => m.message.details.id === chatty.id).length === 2);
    const chattyEvents = h.messages.filter((m) => m.message.details.id === chatty.id).map((m) => m.message.details.kind);
    assert.deepEqual(chattyEvents, ["completed", "update"]);
    assert.match((await h.call("worker_status", { id: chatty.id }))[0].result, /first answer/);

    // message adapts: steer a running worker; continue a finished one; wait for the reply
    const slow = await h.call("worker_spawn", { label: "slow", task: "slow:work" });
    assert.match((await h.run("message", { to: slow.id, text: "hurry" })).content[0].text, /steered/);
    const continued = await h.run("message", { to: spawned.id, text: "and again", wait: true, timeoutMs: 2000 });
    assert.match(continued.content[0].text, /completed[\s\S]*done: and again/);

    // worker_wait returns the settled worker, or times out without harm
    assert.equal((await h.call("worker_wait", { id: spawned.id, timeoutMs: 500 })).state, "completed");
    const pending = await h.call("worker_spawn", { label: "pending", task: "slow:later" });
    assert.match((await h.run("worker_wait", { id: pending.id, timeoutMs: 100 })).content[0].text, /No change within the wait window/);
    assert.equal((await h.call("worker_wait", { id: pending.id, timeoutMs: 2000 })).state, "completed");

    // reload: processes are gone, registry says paused, message continues on --session
    const running = await h.call("worker_spawn", { label: "long", task: "slow:forever" });
    await h.handlers.session_shutdown({}, h.ctx);
    const reloaded = harness(root);
    harnesses.push(reloaded);
    reloaded.entries.push(...h.entries);
    reloaded.handlers.session_start({ reason: "reload" }, reloaded.ctx);
    assert.equal((await reloaded.call("worker_status", { id: running.id }))[0].state, "paused");
    assert.match((await reloaded.run("message", { to: running.id, text: "pick up" })).content[0].text, /continued in its own session/);
    await until(() => reloaded.messages.length === 1);
    assert.match(reloaded.messages[0].message.content, /done: pick up/);

    // a continued worker keeps its model, tools, and contract
    const argsFile = path.join(tmpdir(), `dots-fake-pi-args-${running.id}`);
    const continuedArgs = JSON.parse(readFileSync(argsFile, "utf8"));
    assert.ok(continuedArgs.includes("--session"), "continued on its session");
    assert.ok(continuedArgs.includes("--append-system-prompt"), "contract re-applied");
    rmSync(argsFile, { force: true });
    const scoped = await reloaded.call("worker_spawn", { label: "scoped", task: "quick", tools: ["read"], model: "fake/model:low" });
    await until(() => reloaded.messages.length === 2);
    await reloaded.handlers.session_shutdown({}, reloaded.ctx);
    const third = harness(root);
    harnesses.push(third);
    third.entries.push(...reloaded.entries);
    third.handlers.session_start({ reason: "reload" }, third.ctx);
    await third.run("message", { to: scoped.id, text: "more" });
    const scopedArgs = JSON.parse(readFileSync(path.join(tmpdir(), `dots-fake-pi-args-${scoped.id}`), "utf8"));
    assert.ok(scopedArgs.includes("--tools") && scopedArgs[scopedArgs.indexOf("--tools") + 1] === "read,message");
    assert.ok(scopedArgs.includes("--model") && scopedArgs[scopedArgs.indexOf("--model") + 1] === "fake/model");
    assert.ok(scopedArgs.includes("--thinking") && scopedArgs[scopedArgs.indexOf("--thinking") + 1] === "low");
    rmSync(path.join(tmpdir(), `dots-fake-pi-args-${scoped.id}`), { force: true });

    // confirm dialogs are answered on the protocol's field
    const confirmer = await third.call("worker_spawn", { label: "confirmer", task: "confirm:Proceed?" });
    await until(() => third.messages.some((m) => m.message.details.id === confirmer.id && m.message.details.kind === "asking"));
    await third.run("message", { to: confirmer.id, text: "yes" });
    await until(() => third.messages.some((m) => /confirmed=true/.test(m.message.content)));

    // a child that fails to start is recorded as failed, not left running
    await assert.rejects(third.run("worker_spawn", { label: "doomed", task: "x", model: "broken/model" }), /failed to start/);
    const ghost = (await third.call("worker_status", {})).find((w) => w.label === "doomed");
    assert.equal(ghost.state, "failed");
    assert.match(ghost.error, /Model not found/);

    // stop ends the process; the session file remains addressable; bad addresses are refused
    const victim = await third.call("worker_spawn", { label: "victim", task: "slow:doomed" });
    const stopped = await third.call("worker_stop", { id: victim.id });
    assert.equal(stopped.state, "stopped");
    assert.ok(stopped.session);
    await assert.rejects(third.run("message", { to: "parent", text: "hi" }), /no parent/);
    await assert.rejects(third.run("worker_stop", { id: "" }), /required/);
    await assert.rejects(third.run("worker_stop", { id: "w-" }), /ambiguous/);
  } finally {
    // Always end child processes, or a failed assertion leaves pipes holding the test open.
    for (const h of harnesses) await h.handlers.session_shutdown({}, h.ctx);
    for (const key of Object.keys(process.env)) if (!(key in saved)) delete process.env[key];
    Object.assign(process.env, saved);
    rmSync(root, { recursive: true, force: true });
  }
});
