import assert from "node:assert/strict";
import test from "node:test";
import register, { describe, parseDuration, shouldFire } from "../extensions/wake/index.ts";

// A fake clock: timers fire only when the test advances time, in order.
function fakeClock(start = Date.UTC(2026, 0, 1)) {
  let now = start;
  let seq = 0;
  const timers = new Map();
  return {
    now: () => now,
    setTimeout: (fn, ms) => { const id = ++seq; timers.set(id, { at: now + ms, fn }); return id; },
    clearTimeout: (id) => timers.delete(id),
    pending: () => timers.size,
    async advance(ms) {
      const target = now + ms;
      for (;;) {
        const due = [...timers.entries()].filter(([, t]) => t.at <= target).sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        now = due[1].at;
        timers.delete(due[0]);
        due[1].fn();
        await new Promise((resolve) => setImmediate(resolve)); // let async fire() settle
        await new Promise((resolve) => setImmediate(resolve));
      }
      now = target;
    },
  };
}

function harness(clock, run) {
  const handlers = {};
  const tools = new Map();
  const messages = [];
  const entries = [];
  const pi = {
    on: (name, fn) => { handlers[name] = fn; },
    registerTool: (tool) => tools.set(tool.name, tool),
    registerCommand: () => {},
    registerMessageRenderer: () => {},
    sendMessage: (message, options) => messages.push({ message, options }),
    appendEntry: (customType, data) => entries.push({ type: "custom", customType, data }),
    exec: async () => { throw new Error("exec should be stubbed"); },
  };
  register(pi, { clock, run });
  const ctx = { cwd: "/project", hasUI: false, sessionManager: { getBranch: () => entries } };
  const call = async (name, params) => (await tools.get(name).execute("c", params, undefined, undefined, ctx)).details;
  const text = async (name, params) => (await tools.get(name).execute("c", params, undefined, undefined, ctx)).content[0].text;
  // Pi emits message_start when a follow-up is consumed, including inside an
  // existing run. This does not require a fresh before_agent_start event.
  let consumed = 0;
  const turn = () => {
    for (const { message } of messages.slice(consumed)) handlers.message_start?.({ message: { role: "custom", ...message } }, ctx);
    consumed = messages.length;
  };
  return { handlers, tools, messages, entries, ctx, call, text, turn };
}

test("durations and gating helpers", () => {
  assert.equal(parseDuration("20m"), 1_200_000);
  assert.equal(parseDuration("1.5h"), 5_400_000);
  assert.throws(() => parseDuration("soon"), /duration/);
  assert.equal(describe(90_000), "2m");
  assert.equal(describe(7_200_000), "2h");
  const base = { on: "change", lastOutput: "a", lastCode: 0 };
  assert.equal(shouldFire(base, { stdout: "a", stderr: "", code: 0 }), false);
  assert.equal(shouldFire(base, { stdout: "b", stderr: "", code: 0 }), true);
  assert.equal(shouldFire({ on: "change" }, { stdout: "a", stderr: "", code: 0 }), true, "first observation fires");
  assert.equal(shouldFire({ on: "failure" }, { stdout: "", stderr: "", code: 1 }), true);
  assert.equal(shouldFire({ on: "failure" }, { stdout: "", stderr: "", code: 0 }), false);
  assert.equal(shouldFire({ on: "success" }, { stdout: "", stderr: "", code: 0 }), true);
});

test("one-shot, recurring with limit, no drift, no pile-up, cancel, manual run", async () => {
  const clock = fakeClock();
  const h = harness(clock);
  h.handlers.session_start({ reason: "startup" }, h.ctx);

  const once = await h.call("wake", { text: "check the deploy", in: "20m" });
  assert.equal(once.in, "20m");
  await clock.advance(19 * 60_000);
  assert.equal(h.messages.length, 0);
  await clock.advance(60_000);
  assert.equal(h.messages.length, 1);
  assert.match(h.messages[0].message.content, /check the deploy/);
  assert.deepEqual(h.messages[0].options, { triggerTurn: true, deliverAs: "followUp" });
  assert.equal((await h.call("wake_list", {})).length, 0, "one-shot removes itself");

  const every = await h.call("wake", { text: "poll", every: "10m", limit: 3 });
  assert.equal(every.every, "10m");
  await clock.advance(10 * 60_000);
  assert.equal(h.messages.length, 2);
  assert.match(h.messages[1].message.content, /fire 1 of 3/);
  // Not consumed by a turn yet: the next tick is skipped, not stacked.
  await clock.advance(10 * 60_000);
  assert.equal(h.messages.length, 2);
  h.turn();
  await clock.advance(10 * 60_000);
  assert.equal(h.messages.length, 3);
  assert.match(h.messages[2].message.content, /fire 2 of 3/);
  h.turn();
  await clock.advance(10 * 60_000);
  assert.equal(h.messages.length, 4);
  assert.match(h.messages[3].message.content, /fire 3 of 3/);
  h.turn();
  await clock.advance(30 * 60_000);
  assert.equal(h.messages.length, 4, "limit reached");
  assert.equal((await h.call("wake_list", {})).length, 0);

  const loop = await h.call("wake", { text: "tick", every: "5m" });
  assert.equal(loop.limit, 50);
  await clock.advance(5 * 60_000);
  h.turn();
  assert.equal(h.messages.length, 5);
  assert.match(await h.text("wake_run", { id: loop.id }), /fired \(2 total\)/);
  assert.match(h.messages[5].message.content, /manual/);
  assert.match(await h.text("wake_cancel", { id: loop.id.slice(0, 4) }), /cancelled after 2/);
  await clock.advance(60 * 60_000);
  assert.equal(h.messages.length, 6);

  await assert.rejects(h.call("wake", { text: "x", every: "10s" }), /at least/);
  await assert.rejects(h.call("wake", { text: "x", in: "1m", every: "5m" }), /exactly one/);
  await assert.rejects(h.call("wake", { text: "x", at: "2001-01-01T00:00:00Z" }), /past/);
  await assert.rejects(h.call("wake", { text: "x", in: "5m", on: "failure" }), /requires check/);
  assert.equal(clock.pending(), 0, "no timers left behind");
});

test("check-gated wakes only wake on the chosen condition and carry output", async () => {
  const clock = fakeClock();
  const runs = [];
  let output = { stdout: "pending\n", stderr: "", code: 0 };
  const h = harness(clock, async (command, cwd) => { runs.push([command, cwd]); return output; });
  h.handlers.session_start({ reason: "startup" }, h.ctx);

  const ci = await h.call("wake", { text: "CI changed; inspect", every: "5m", check: "gh run list --limit 1", on: "change" });
  await clock.advance(5 * 60_000);
  assert.equal(runs.length, 1);
  assert.deepEqual(runs[0], ["gh run list --limit 1", "/project"]);
  assert.equal(h.messages.length, 1, "first observation fires");
  assert.match(h.messages[0].message.content, /exit: 0\npending/);
  h.turn();
  await clock.advance(5 * 60_000);
  assert.equal(h.messages.length, 1, "same output: silent");
  output = { stdout: "failure\n", stderr: "", code: 0 };
  await clock.advance(5 * 60_000);
  assert.equal(h.messages.length, 2, "changed output fires");
  assert.match(h.messages[1].message.content, /failure/);
  h.turn();

  const red = await h.call("wake", { text: "build broke", every: "2m", check: "make check", on: "failure" });
  output = { stdout: "ok", stderr: "", code: 0 };
  await clock.advance(2 * 60_000);
  assert.equal(h.messages.length, 2, "green build: silent");
  output = { stdout: "", stderr: "boom", code: 2 };
  await clock.advance(2 * 60_000);
  assert.equal(h.messages.length, 3);
  assert.match(h.messages[2].message.content, /exit: 2\nstderr: boom/);
  h.turn();
  assert.match(await h.text("wake_run", { id: red.id }), /fired/);
  assert.equal(h.messages.length, 4, "manual run ignores the gate");
  await h.call("wake_cancel", { id: ci.id });
  await h.call("wake_cancel", { id: red.id });
});

test("reload re-arms from the session; a missed one-shot fires once, late", async () => {
  const clock = fakeClock();
  const h = harness(clock);
  h.handlers.session_start({ reason: "startup" }, h.ctx);
  const soon = await h.call("wake", { text: "soon", in: "5m" });
  const later = await h.call("wake", { text: "later", in: "60m" });
  const cadence = await h.call("wake", { text: "cadence", every: "15m", limit: 10 });
  h.handlers.session_shutdown({}, h.ctx);
  assert.equal(clock.pending(), 0);

  // Away for 40 minutes, then reload the same session.
  await clock.advance(40 * 60_000);
  const h2 = harness(clock);
  h2.entries.push(...h.entries);
  h2.handlers.session_start({ reason: "reload" }, h2.ctx);
  await clock.advance(0);
  const fired = h2.messages.map((m) => m.message.content);
  assert.equal(fired.filter((c) => /\bsoon\b/.test(c)).length, 1, "missed one-shot fires once");
  assert.match(fired.find((c) => /\bsoon\b/.test(c)), /late by 35m/);
  assert.equal(fired.filter((c) => /\bcadence\b/.test(c)).length, 1, "missed recurring collapses to one fire");
  assert.equal(fired.filter((c) => /\blater\b/.test(c)).length, 0);
  const listed = await h2.call("wake_list", {});
  assert.deepEqual(listed.map((w) => w.id).sort(), [cadence.id, later.id].sort());
  assert.equal(listed.find((w) => w.id === cadence.id).in, "15m", "continues from now, not from the missed slots");
  h2.turn();
  await clock.advance(20 * 60_000);
  assert.equal(h2.messages.filter((m) => /\blater\b/.test(m.message.content)).length, 1);
  assert.equal(h2.messages.filter((m) => /\bcadence\b/.test(m.message.content)).length, 2);
  h2.handlers.session_shutdown({}, h2.ctx);
  assert.equal(clock.pending(), 0);
});

test("in-flight checks cannot publish or re-arm after cancellation or shutdown", async () => {
  for (const action of ["cancel", "shutdown", "replace"]) {
    const clock = fakeClock();
    let complete;
    const h = harness(clock, () => new Promise((resolve) => { complete = resolve; }));
    h.handlers.session_start({}, h.ctx);
    const wake = await h.call("wake", { text: "check", every: "1m", check: "true" });
    await clock.advance(60_000);
    assert.equal(typeof complete, "function");
    if (action === "cancel") await h.call("wake_cancel", { id: wake.id });
    else h.handlers.session_shutdown({}, h.ctx);
    if (action === "replace") {
      h.entries.length = 0;
      h.handlers.session_start({}, h.ctx);
    }
    const saved = h.entries.length;
    complete({ stdout: "done", stderr: "", code: 0 });
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(h.messages.length, 0, action);
    assert.equal(h.entries.length, saved, action);
    assert.equal(clock.pending(), 0, action);
  }
});

test("a gated missed wake advances on restore instead of disappearing", async () => {
  const clock = fakeClock();
  const run = async () => ({ stdout: "same", stderr: "", code: 0 });
  const h = harness(clock, run);
  h.handlers.session_start({}, h.ctx);
  await h.call("wake", { text: "poll", every: "1m", check: "true" });
  await clock.advance(60_000);
  h.handlers.session_shutdown({}, h.ctx);
  await clock.advance(120_000);
  const restored = harness(clock, run);
  restored.entries.push(...h.entries);
  restored.handlers.session_start({}, restored.ctx);
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(restored.messages.length, 0);
  assert.equal(clock.pending(), 1);
  assert.equal((await restored.call("wake_list", {}))[0].in, "1m");
  restored.handlers.session_shutdown({}, restored.ctx);
});

test("30-day wakes chunk timers within Node's signed-int32 delay limit", async () => {
  const clock = fakeClock();
  const setTimeout = clock.setTimeout;
  clock.setTimeout = (fn, ms) => {
    assert.ok(ms <= 2_147_483_647);
    return setTimeout(fn, ms);
  };
  const h = harness(clock);
  h.handlers.session_start({}, h.ctx);
  await h.call("wake", { text: "later", in: "30d" });
  await clock.advance(2_147_483_647);
  assert.equal(h.messages.length, 0);
  assert.equal(clock.pending(), 1);
  await clock.advance(30 * 86400_000 - 2_147_483_647);
  assert.equal(h.messages.length, 1);
});

test("manual checks own the timer, reject overlap, and enforce the shared limit", async () => {
  const clock = fakeClock();
  let complete;
  let runs = 0;
  const h = harness(clock, () => { runs++; return new Promise((resolve) => { complete = resolve; }); });
  await h.handlers.session_start({}, h.ctx);
  const wake = await h.call("wake", { text: "poll", every: "1m", limit: 2, check: "true" });
  await clock.advance(30_000);
  const manual = h.text("wake_run", { id: wake.id });
  await assert.rejects(h.call("wake_run", { id: wake.id }), /check in progress/);
  await clock.advance(60_000);
  assert.equal(runs, 1, "the scheduled slot cannot overlap the manual check");
  complete({ stdout: "ok", stderr: "", code: 0 });
  assert.match(await manual, /fired \(1 total\)/);
  await clock.advance(0);
  assert.equal(h.messages.length, 1, "the missed scheduled slot is not stacked");
  assert.equal((await h.call("wake_list", {}))[0].in, "30s");
  h.turn();
  const final = h.text("wake_run", { id: wake.id });
  complete({ stdout: "ok", stderr: "", code: 0 });
  assert.match(await final, /fired \(2 total\)/);
  assert.equal(clock.pending(), 0);
  assert.equal((await h.call("wake_list", {})).length, 0);
  assert.match(await h.text("wake_run", { id: wake.id }), /did not fire/);
  assert.equal(runs, 2);
});

test("follow-up consumption releases only the matching fire without a new run", async () => {
  const clock = fakeClock();
  const h = harness(clock);
  await h.handlers.session_start({}, h.ctx);
  const wake = await h.call("wake", { text: "poll", every: "1m", limit: 4 });
  await clock.advance(60_000);
  await h.call("wake_run", { id: wake.id });
  const consume = (index) => h.handlers.message_start({ message: { role: "custom", ...h.messages[index].message } });
  consume(0);
  await clock.advance(60_000);
  assert.equal(h.messages.length, 2, "an older consumed fire does not release the newest pending one");
  h.handlers.message_start({ message: { role: "user", content: "unrelated" } });
  await clock.advance(60_000);
  assert.equal(h.messages.length, 2);
  consume(1);
  await clock.advance(60_000);
  assert.equal(h.messages.length, 3, "consumption inside the same agent run releases the pending slot");
  h.handlers.session_shutdown({}, h.ctx);
});

test("checks persist a bounded digest rather than complete output", async () => {
  const clock = fakeClock();
  const h = harness(clock, async () => ({ stdout: "x".repeat(200_000), stderr: "", code: 0 }));
  await h.handlers.session_start({}, h.ctx);
  await h.call("wake", { text: "poll", every: "1m", check: "verbose fixture" });
  await clock.advance(60_000);
  h.turn();
  await clock.advance(60_000);
  assert.equal(h.messages.length, 1);
  assert.ok(h.entries.every((entry) => entry.data.lastOutput === undefined));
  assert.ok(h.entries.every((entry) => JSON.stringify(entry).length < 1000));
  assert.match(h.entries.at(-1).data.lastDigest, /^[0-9a-f]{64}:[0-9a-f]{64}:$/);
  h.handlers.session_shutdown({}, h.ctx);
});

test("cancelling a manual check aborts its runner and does not claim a fire", async () => {
  const clock = fakeClock();
  let complete;
  let signal;
  const h = harness(clock, (_command, _cwd, passed) => {
    signal = passed;
    return new Promise((resolve) => { complete = resolve; });
  });
  await h.handlers.session_start({}, h.ctx);
  const wake = await h.call("wake", { text: "later", in: "1m", check: "true" });
  const pending = h.text("wake_run", { id: wake.id });
  await h.call("wake_cancel", { id: wake.id });
  assert.equal(signal.aborted, true);
  complete({ stdout: "ignored", stderr: "", code: 0 });
  assert.match(await pending, /did not fire/);
  assert.equal(h.messages.length, 0);
  assert.equal(clock.pending(), 0);
});
