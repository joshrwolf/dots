import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import register from "../../extensions/workers/index.ts";
import { Registry, openRegistry } from "../../extensions/workers/registry.ts";

// The extension's lifecycle against a scripted `pi`: spawn -> event -> wake,
// message in every direction, late settles as updates, wait, reload -> paused
// -> continue, stop. No model, no credentials.
const FAKE_PI = `#!/usr/bin/env node
const args = process.argv.slice(2);
if (args.includes("--model") && args[args.indexOf("--model") + 1] === "broken/model") { process.stderr.write("Model not found\\n"); process.exit(2); }
require("fs").writeFileSync(require("path").join(require("os").tmpdir(), "dots-fake-pi-args-" + (process.env.DOTS_WORKER_ID || "root")), JSON.stringify(args));
const sessionFile = (args.includes("--session") ? args[args.indexOf("--session") + 1] : args[args.indexOf("--session-dir") + 1] + "/session.jsonl");
const out = (o) => process.stdout.write(JSON.stringify(o) + "\\n");
let active = false;
let pendingSettle;
const settle = (text) => { active = false; global.last = text; out({ type: "agent_settled", aborted: false }); };
let buffer = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  buffer += chunk;
  let i;
  while ((i = buffer.indexOf("\\n")) !== -1) {
    const cmd = JSON.parse(buffer.slice(0, i)); buffer = buffer.slice(i + 1);
    const ok = (data) => out({ id: cmd.id, type: "response", command: cmd.type, success: true, data });
    if (cmd.type === "prompt") {
      if (cmd.message === "reject") { out({ id: cmd.id, type: "response", success: false, error: "rejected continuation" }); continue; }
      if (cmd.message === "handled") { ok({ disposition: "handled" }); continue; }
      if (cmd.message === "no-ack") { active = true; continue; }
      if (active && cmd.streamingBehavior === "steer") {
        ok({ disposition: "queued" });
        clearTimeout(pendingSettle);
        pendingSettle = setTimeout(() => settle("steered: " + cmd.message), 50);
        continue;
      }
      active = true;
      out({ type: "agent_start" });
      if (cmd.message === "delayed-ack") { setTimeout(() => { ok({ disposition: "started" }); settle("delayed done"); }, 150); continue; }
      ok({ disposition: "started" });
      if (cmd.message === "provider-error") {
        out({ type: "message_end", message: { role: "assistant", content: [], stopReason: "error", errorMessage: "provider unavailable" } });
        settle(""); continue;
      }
      out({ type: "tool_execution_start", toolCallId: "t", toolName: "bash", args: {} });
      out({ type: "message_update", usage: { totalTokens: 7 }, assistantMessageEvent: { type: "text_delta", delta: "x" } });
      const m = cmd.message;
      global.ignoreAbort = m === "slow:ignore-abort";
      if (m === "delay-result") { global.delayRead = true; settle("old result"); continue; }
      if (m === "two-questions") {
        out({ type: "extension_ui_request", id: "q1", method: "input", title: "first question" });
        out({ type: "extension_ui_request", id: "q2", method: "input", title: "second question" });
        continue;
      }
      if (m === "crash-question") {
        out({ type: "extension_ui_request", id: "dead-question", method: "input", title: "question before crash", timeout: 600000 });
        setTimeout(() => process.exit(1), 50);
        continue;
      }
      if (m === "expired-question") {
        out({ type: "extension_ui_request", id: "expired", method: "input", title: "expires", timeout: 30 });
        setTimeout(() => out({ type: "tool_execution_end", toolName: "message" }), 40);
        continue;
      }
      if (m.startsWith("ask:")) { out({ type: "extension_ui_request", id: "ui-9", method: "input", title: m.slice(4) }); return; }
      if (m.startsWith("confirm:")) { out({ type: "extension_ui_request", id: "ui-c", method: "confirm", title: m.slice(8) }); return; }
      if (m.startsWith("slow:")) { pendingSettle = setTimeout(() => settle("slow done"), 400); return; }
      if (m.startsWith("long:")) { settle("OUTCOME first line\\n" + Array.from({ length: 200 }, (_, i) => "detail line " + i).join("\\n")); return; }
      if (m.startsWith("spontaneous:")) { settle("first answer"); setTimeout(() => { active = true; out({ type: "agent_start" }); setTimeout(() => settle("later chatter"), 80); }, 120); return; }
      if (m.startsWith("tell-parent:")) { out({ type: "extension_ui_request", id: "n1", method: "notify", message: "dots.message:" + JSON.stringify({ to: "parent", text: m.slice(12) }) }); settle("told"); return; }
      if (m.startsWith("tell-sibling:")) { const [to, ...rest] = m.slice(13).split(" "); out({ type: "extension_ui_request", id: "n2", method: "notify", message: "dots.message:" + JSON.stringify({ to, text: rest.join(" ") }) }); settle("relayed"); return; }
      settle("done: " + m);
    } else if (cmd.type === "extension_ui_response") {
      if (cmd.id === "q1") { out({ type: "tool_execution_end", toolName: "message" }); continue; }
      if (cmd.id === "q2") { settle("both answered"); continue; }
      if (cmd.id === "expired") continue;
      settle(cmd.id === "ui-c" ? "confirmed=" + cmd.confirmed : "answered with " + cmd.value);
    } else if (cmd.type === "steer") {
      ok({ disposition: "queued" }); setTimeout(() => settle("steered: " + cmd.message), 50);
    } else if (cmd.type === "abort") {
      if (global.ignoreAbort) return;
      ok({}); out({ type: "agent_settled", aborted: true });
    } else if (cmd.type === "get_last_assistant_text") {
      if (global.delayRead) {
        global.delayRead = false;
        require("fs").writeFileSync(sessionFile + ".reading", "ready");
        const text = global.last;
        setTimeout(() => ok({ text }), 150);
        continue;
      }
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
  const commands = new Map();
  const messages = [];
  const entries = [];
  const pi = {
    on: (name, fn) => { handlers[name] = fn; },
    registerTool: (tool) => tools.set(tool.name, tool),
    registerCommand: (name, command) => commands.set(name, command),
    registerMessageRenderer: () => {},
    sendMessage: (message, options) => messages.push({ message, options }),
    appendEntry: (customType, data) => entries.push({ type: "custom", customType, data }),
  };
  register(pi);
  const ctx = { cwd: root, hasUI: false, sessionManager: { getBranch: () => entries } };
  const run = (name, params) => tools.get(name).execute("call", params, undefined, undefined, ctx);
  const call = async (name, params) => (await run(name, params)).details;
  return { handlers, tools, commands, messages, entries, ctx, call, run };
}

test("parent questions forward deadlines and sibling relays never pretend to wait", async () => {
  const previous = process.env.DOTS_WORKER_ID;
  process.env.DOTS_WORKER_ID = "w-test";
  try {
    const h = harness(tmpdir());
    h.ctx.ui = {
      async input(_text, _placeholder, options) {
        assert.equal(options.timeout, 1234);
        return undefined;
      },
      notify() { assert.fail("unsupported waiting relay must not be sent"); },
    };
    assert.match((await h.run("message", { to: "parent", text: "Approve?", wait: true, timeoutMs: 1234 })).content[0].text, /No decision was approved/);
    await assert.rejects(h.run("message", { to: "w-sibling", text: "hello", wait: true }), /not supported for sibling/);
    let answer;
    h.ctx.ui.input = () => new Promise((resolve) => { answer = resolve; });
    const first = h.run("message", { to: "parent", text: "first", wait: true });
    await assert.rejects(h.run("message", { to: "parent", text: "second", wait: true }), /already awaiting/);
    answer("yes");
    await first;
  } finally {
    if (previous === undefined) delete process.env.DOTS_WORKER_ID;
    else process.env.DOTS_WORKER_ID = previous;
  }
});

test("worker lifecycle against a scripted pi", { timeout: 15_000 }, async (t) => {
  const intervals = new Map();
  t.mock.method(globalThis, "setInterval", (fn) => {
    const handle = {};
    intervals.set(handle, fn);
    return handle;
  });
  t.mock.method(globalThis, "clearInterval", (handle) => intervals.delete(handle));
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
    let stale = false;
    let renders = 0;
    const warnings = [];
    h.ctx.hasUI = true;
    h.ctx.mode = "tui";
    h.ctx.model = { provider: "fake", id: "inherited" };
    h.ctx.thinkingLevel = "low";
    Object.defineProperty(h.ctx, "ui", { get() {
      assert.equal(stale, false, "must not access the old UI after reload");
      return {
        setStatus() {},
        setWidget(_key, factory) { factory?.({ requestRender() { renders++; } }, {}); },
        notify(text) { warnings.push(text); },
      };
    } });
    h.handlers.session_start({ reason: "startup" }, h.ctx);
    assert.deepEqual([...h.tools.keys()].sort(), ["message", "worker_spawn", "worker_status", "worker_stop", "worker_wait"]);

    assert.match((await h.run("worker_wait", {})).content[0].text, /No workers/);

    // spawn -> completion event wakes the owner with the child's final text
    const spawned = await h.call("worker_spawn", { label: "one", task: "count things" });
    assert.equal(spawned.state, "running");
    assert.equal(intervals.size, 1, "live interactive workers start the ticker");
    await until(() => h.messages.length === 1);
    assert.equal(intervals.size, 0, "last completion stops the ticker");
    const inheritedArgs = JSON.parse(readFileSync(path.join(tmpdir(), `dots-fake-pi-args-${spawned.id}`), "utf8"));
    assert.equal(inheritedArgs[inheritedArgs.indexOf("--model") + 1], "fake/inherited");
    assert.equal(inheritedArgs[inheritedArgs.indexOf("--thinking") + 1], "low");
    assert.equal(h.messages[0].message.details.kind, "completed");
    assert.match(h.messages[0].message.content, /done: count things/);
    assert.deepEqual(h.messages[0].options, { triggerTurn: true, deliverAs: "steer" });
    const one = await h.call("worker_status", { id: spawned.id });
    assert.ok(one.session.endsWith("session.jsonl"));
    const oneText = (await h.run("worker_status", { id: spawned.id })).content[0].text;
    assert.match(oneText, /one · completed · age \d+s · \d+ tok · \d+ calls?/);
    assert.match(oneText, /result:\n.*done: count things/s);
    assert.match((await h.run("worker_status", {})).content[0].text, /result: done: count things/, "overview previews the first result line");

    // Observation failure must not prevent the authoritative worker lifecycle.
    t.mock.method(Registry.prototype, "event", () => { throw new Error("test registry unavailable"); }, { times: 1 });
    // child asks -> asking event -> message answers it -> completion
    const asker = await h.call("worker_spawn", { label: "asker", task: "ask:Which base?" });
    await until(() => h.messages.length === 2);
    assert.equal(h.messages[1].message.details.kind, "asking");
    assert.equal(warnings.length, 1);
    assert.match(warnings[0], /registry disabled/);
    assert.match((await h.run("worker_wait", { all: true })).content[0].text, /asking.*answer it/);
    assert.match(h.messages[1].message.content, /Which base\?/);
    const answered = await h.run("message", { to: asker.id, text: "main" });
    assert.match(answered.content[0].text, /sent an answer to its question/);
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
    assert.match((await h.call("worker_status", { id: verbose.id })).result, /detail line 199/);

    // a settle we did not ask for is an update and keeps the real result
    const chatty = await h.call("worker_spawn", { label: "chatty", task: "spontaneous:go" });
    await until(() => h.messages.filter((m) => m.message.details.id === chatty.id).length === 2);
    const chattyEvents = h.messages.filter((m) => m.message.details.id === chatty.id).map((m) => m.message.details.kind);
    assert.deepEqual(chattyEvents, ["completed", "update"]);
    assert.match((await h.call("worker_status", { id: chatty.id })).result, /first answer/);

    // message adapts: steer a running worker; continue a finished one; wait for the reply
    const slow = await h.call("worker_spawn", { label: "slow", task: "slow:work" });
    assert.match((await h.run("message", { to: slow.id, text: "hurry" })).content[0].text, /steered/);
    const continued = await h.run("message", { to: spawned.id, text: "and again", wait: true, timeoutMs: 2000 });
    assert.match(continued.content[0].text, /completed[\s\S]*done: and again/);

    // worker_wait returns the settled worker, or times out without harm
    assert.equal((await h.call("worker_wait", { id: spawned.id, timeoutMs: 500 })).state, "completed");
    const pending = await h.call("worker_spawn", { label: "pending", task: "slow:later" });
    assert.match((await h.run("worker_wait", { id: pending.id, timeoutMs: 100 })).content[0].text, /No change within the wait window/);
    const seenBefore = h.messages.length;
    const waited = await h.run("worker_wait", { id: pending.id, timeoutMs: 2000 });
    assert.match(waited.content[0].text, /completed[\s\S]*slow done/, "wait returns the full result");
    await until(() => h.messages.length >= seenBefore + 1);
    const dup = h.messages.findLast((m) => m.message.details.id === pending.id);
    assert.match(dup.message.content, /\[delivered via worker_wait\]/, "the event keeps the card but not the text");
    assert.doesNotMatch(dup.message.content, /slow done[\s\S]*slow done/);
    assert.equal(dup.options.triggerTurn, false, "no extra turn for a result the model already has");

    // all: true collects every live worker
    const p1 = await h.call("worker_spawn", { label: "p1", task: "slow:one" });
    const p2 = await h.call("worker_spawn", { label: "p2", task: "slow:two" });
    const everyone = await h.run("worker_wait", { all: true, timeoutMs: 5000 });
    assert.match(everyone.content[0].text, new RegExp(`${p1.id} p1 completed[\\s\\S]*slow done`));
    assert.match(everyone.content[0].text, new RegExp(`${p2.id} p2 completed[\\s\\S]*slow done`));

    // reload: processes are gone, registry says paused, message continues on --session
    const running = await h.call("worker_spawn", { label: "long", task: "slow:forever" });
    assert.equal(intervals.size, 1);
    const queuedTick = [...intervals.values()][0];
    const outstandingWait = h.run("worker_wait", { id: running.id });
    const shutdown = h.handlers.session_shutdown({}, h.ctx);
    assert.equal(intervals.size, 0, "shutdown clears the ticker before awaiting children");
    await shutdown;
    await outstandingWait;
    stale = true;
    const rendersBefore = renders;
    assert.doesNotThrow(() => queuedTick(), "even a queued callback cannot use a stale ctx");
    assert.equal(renders, rendersBefore, "shutdown releases the render callback");
    const reloaded = harness(root);
    harnesses.push(reloaded);
    reloaded.entries.push(...h.entries);
    reloaded.handlers.session_start({ reason: "reload" }, reloaded.ctx);
    assert.equal((await reloaded.call("worker_status", { id: running.id })).state, "paused");
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

    const emptyTools = await third.call("worker_spawn", { label: "empty-tools", task: "quick", tools: [] });
    const emptyArgsFile = path.join(tmpdir(), `dots-fake-pi-args-${emptyTools.id}`);
    const emptyArgs = JSON.parse(readFileSync(emptyArgsFile, "utf8"));
    assert.equal(emptyArgs[emptyArgs.indexOf("--tools") + 1], "message", "empty allowlist is not the default tool set");
    rmSync(emptyArgsFile, { force: true });

    // confirm dialogs are answered on the protocol's field
    const confirmer = await third.call("worker_spawn", { label: "confirmer", task: "confirm:Proceed?" });
    await until(() => third.messages.some((m) => m.message.details.id === confirmer.id && m.message.details.kind === "asking"));
    assert.match((await third.run("worker_wait", { all: true })).content[0].text, /asking.*answer it/);
    await third.run("message", { to: confirmer.id, text: "yes" });
    await until(() => third.messages.some((m) => /confirmed=true/.test(m.message.content)));

    // a child that fails to start is recorded as failed, not left running
    await assert.rejects(third.run("worker_spawn", { label: "doomed", task: "x", model: "broken/model" }), /failed to start/);
    const ghost = (await third.call("worker_status", {})).find((w) => w.label === "doomed");
    assert.equal(ghost.state, "failed");
    assert.match(ghost.error, /Model not found/);

    // stop ends the process; the session file remains addressable; bad addresses are refused
    const victim = await third.call("worker_spawn", { label: "victim", task: "slow:ignore-abort" });
    const stoppedResult = await third.run("worker_stop", { id: victim.id });
    const stopped = stoppedResult.details;
    assert.match(stoppedResult.content[0].text, /Saved session retained.*message can continue/);
    assert.equal(stopped.state, "stopped");
    assert.equal(stopped.processState, "closed");
    assert.ok(stopped.session);
    await assert.rejects(third.run("message", { to: "parent", text: "hi" }), /no parent/);
    await assert.rejects(third.run("worker_stop", { id: "" }), /required|needs id/);
    await assert.rejects(third.run("worker_stop", { id: "w-" }), /ambiguous/);

    // stop all: every live worker of this session, finished ones untouched
    const s1 = await third.call("worker_spawn", { label: "slow-1", task: "slow:a" });
    const s2 = await third.call("worker_spawn", { label: "slow-2", task: "slow:b" });
    const all = await third.call("worker_stop", { all: true });
    assert.deepEqual(all.stopped.map((w) => w.id).sort(), [s1.id, s2.id].sort());
    assert.ok(all.stopped.every((w) => w.state === "stopped"));
    assert.equal((await third.call("worker_stop", { all: true })).stopped.length, 0);
    assert.equal((await third.call("worker_status", {})).find((w) => w.label === "victim").state, "stopped");

    // A waiting delivery must not consume the previous run's in-flight settle.
    const prior = await third.call("worker_spawn", { label: "prior", task: "delay-result" });
    await until(() => existsSync(`${prior.session}.reading`));
    const fresh = await third.run("message", { to: prior.id, text: "new result", wait: true, timeoutMs: 2000 });
    assert.match(fresh.content[0].text, /done: new result/);
    assert.doesNotMatch(fresh.content[0].text, /old result/);

    const questions = await third.call("worker_spawn", { label: "questions", task: "two-questions" });
    await until(() => third.messages.filter((m) => m.message.details.id === questions.id && m.message.details.kind === "asking").length === 2);
    assert.equal((await third.call("worker_status", { id: questions.id })).pendingQuestions.length, 2);
    const nextQuestion = await third.run("message", { to: questions.id, text: "one", wait: true, timeoutMs: 2000 });
    assert.match(nextQuestion.content[0].text, /second question/);
    assert.equal((await third.call("worker_status", { id: questions.id })).question, "second question");
    await third.call("message", { to: questions.id, text: "two", wait: true, timeoutMs: 2000 });
    assert.equal((await third.call("worker_status", { id: questions.id })).result, "both answered");
    const expired = await third.call("worker_spawn", { label: "expired", task: "expired-question" });
    await sleep(80);
    const afterExpiry = await third.run("message", { to: expired.id, text: "after expiry", wait: true, timeoutMs: 2000 });
    assert.match(afterExpiry.content[0].text, /steered: after expiry/);

    const crashed = await third.call("worker_spawn", { label: "crashed", task: "crash-question" });
    await until(() => third.messages.some((m) => m.message.details.id === crashed.id && m.message.details.kind === "failed"));
    assert.equal((await third.call("worker_status", { id: crashed.id })).question, undefined);
    const resumed = await third.run("message", { to: crashed.id, text: "recover after crash", wait: true, timeoutMs: 2000 });
    assert.match(resumed.content[0].text, /done: recover after crash/);

    // Accepted prompts can fail at the provider; no final text is not success.
    const provider = await third.call("worker_spawn", { label: "provider", task: "provider-error" });
    await until(() => third.messages.some((m) => m.message.details.id === provider.id));
    assert.equal((await third.call("worker_status", { id: provider.id })).state, "failed");
    assert.match((await third.run("worker_status", { id: provider.id })).content[0].text, /provider unavailable/);

    // Failed or handled continuations retire the process, remain resumable,
    // and clear their error when a later explicit retry succeeds.
    for (const text of ["reject", "handled"]) {
      await assert.rejects(third.run("message", { to: provider.id, text }), /rejected continuation|without starting/);
      assert.equal((await third.call("worker_status", { id: provider.id })).state, "failed");
      await third.run("message", { to: provider.id, text: "recover", wait: true, timeoutMs: 2000 });
      const recovered = await third.call("worker_status", { id: provider.id });
      assert.equal(recovered.state, "completed");
      assert.equal(recovered.error, undefined);
    }

    // Two continuations of a paused child serialize and share one resumed run.
    await third.call("worker_stop", { id: provider.id });
    await Promise.all([
      third.run("message", { to: provider.id, text: "delayed-ack" }),
      third.run("message", { to: provider.id, text: "second" }),
    ]);
    await third.run("worker_wait", { id: provider.id, timeoutMs: 2000 });
    assert.match((await third.call("worker_status", { id: provider.id })).result, /second/);

    // Stop preempts an unacknowledged message and invalidates queued messages.
    const blocked = assert.rejects(third.run("message", { to: provider.id, text: "no-ack" }), /closing|cancelled/);
    const queued = assert.rejects(third.run("message", { to: provider.id, text: "must not resume" }), /cancelled/);
    await until(() => third.entries.findLast((entry) => entry.data.id === provider.id)?.data.state === "running");
    await third.call("worker_stop", { id: provider.id });
    await Promise.all([blocked, queued]);
    assert.equal((await third.call("worker_status", { id: provider.id })).state, "stopped");

    const idleCleanup = await third.run("worker_stop", { all: true });
    assert.equal(idleCleanup.details.stopped.length, 0, "there was no active work");
    assert.ok(idleCleanup.details.closed.includes(questions.id), "retained idle processes were still closed");
    assert.match(idleCleanup.content[0].text, /Closed \d+ owned processes/);
    assert.match(idleCleanup.content[0].text, /Finished results and saved sessions are retained/);
    assert.equal((await third.call("worker_status", { id: questions.id })).state, "completed");
    assert.equal((await third.call("worker_status", { id: questions.id })).processState, "closed");
    assert.match((await third.run("worker_status", { id: questions.id })).content[0].text, /process closed/);
    assert.match((await third.run("worker_stop", { all: true })).content[0].text, /No owned processes needed closing/);

    // Completed owners/intermediates stay visible while descendants are live;
    // footer counts only active workers, not their contextual ancestor rows.
    let fleet = "";
    third.ctx.mode = "tui";
    third.ctx.hasUI = true;
    third.ctx.ui = { setStatus(_key, text) { fleet = text; }, setWidget() {}, notify() {} };
    const owner = await third.call("worker_spawn", { label: "owner", task: "slow:owner" });
    const registry = await openRegistry(path.join(root, "agent/dots/workers.db"), (m) => assert.fail(m));
    try {
      const row = (id, parent, state) => ({ id, parent, root: owner.id, owner_session: null, label: id, state, cwd: root, model: null, activity: null, failed: null, question: state === "asking" ? "Which option?" : null, calls: 0, tokens: 0, started_at: Date.now(), last_event_at: Date.now(), updated_at: Date.now(), session_file: null, pid: process.pid });
      registry.upsert(row("w-middle", owner.id, "completed"));
      registry.upsert(row("w-leaf", "w-middle", "asking"));
      registry.upsert(row("w-unrelated", null, "running"));
      await third.run("worker_wait", { id: owner.id, timeoutMs: 2000 });
      assert.match(fleet, /workers 1.*\?1/);
      assert.equal(intervals.size, 1, "descendants keep the observation tick alive");
      assert.match((await third.run("worker_status", { id: "w-leaf" })).content[0].text, /Which option/);
      await assert.rejects(third.run("worker_status", { id: "w-unrelated" }), /no worker/);
      const stoppedTree = await third.call("worker_stop", { all: true });
      assert.ok(stoppedTree.stopped.some((w) => w.id === owner.id));
      assert.equal((await third.call("worker_status", { id: owner.id })).state, "completed", "closing retained owners preserves completed results");
      registry.upsert(row("w-leaf", "w-middle", "paused"));
      for (const tick of intervals.values()) tick();
      assert.equal(intervals.size, 0);
    } finally { registry.close(); }

    // A command already stopping children must not notify through a retired UI.
    await third.call("worker_spawn", { label: "closing", task: "slow:closing" });
    const stoppingCommand = third.commands.get("workers").handler("stop", third.ctx);
    const shutdownDuringStop = third.handlers.session_shutdown({}, third.ctx);
    third.ctx.ui.notify = () => assert.fail("old command UI used after shutdown");
    await Promise.all([stoppingCommand, shutdownDuringStop]);
  } finally {
    // Always end child processes, or a failed assertion leaves pipes holding the test open.
    for (const h of harnesses) await h.handlers.session_shutdown({}, h.ctx);
    const ids = new Set(harnesses.flatMap((h) => h.entries.map((entry) => entry.data.id)));
    for (const id of ids) rmSync(path.join(tmpdir(), `dots-fake-pi-args-${id}`), { force: true });
    for (const key of Object.keys(process.env)) if (!(key in saved)) delete process.env[key];
    Object.assign(process.env, saved);
    rmSync(root, { recursive: true, force: true });
  }
});
