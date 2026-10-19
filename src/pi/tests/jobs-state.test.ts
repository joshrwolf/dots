import assert from "node:assert/strict";
import { mkdtempSync, readdirSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import test from "node:test";
import { JobsRuntime } from "../extensions/jobs/runtime.ts";
import { emptyFacts, restore, type Job } from "../extensions/jobs/protocol.ts";

export function exampleJob(): Job {
  return { version: 1, id: "j-00000000-0000-0000-0000-000000000001", command: "exit 0", cwd: "/tmp", directory: "/tmp/neutral-job", timeoutMs: 1000, startedAt: 1, finishedAt: 2, state: "finished", stopIntent: null, facts: emptyFacts(), supervisorExit: { code: null, signal: "SIGKILL" }, observationLoss: null, artifactsEvicted: false };
}
async function until(predicate: () => boolean, timeout = 8000): Promise<void> {
  const end = Date.now() + timeout;
  while (!predicate()) { if (Date.now() > end) throw new Error("runtime fixture deadline"); await delay(25); }
}

test("restore validates bounded records and never restores control", () => {
  const j = exampleJob();
  j.state = "running";
  const restored = restore(j);
  assert.equal(restored?.state, "unknown");
  assert.match(restored?.observationLoss ?? "", /ownership/);
  assert.equal(j.state, "running");
  assert.equal(restore({ ...j, command: "x".repeat(8193) }), undefined);
  assert.equal(restore({ ...j, facts: { capture: {} } }), undefined);
  assert.equal(restore({ ...j, facts: { ...j.facts, root: {} } }), undefined);
  const runtime = new JobsRuntime({ root: "/unused", persist() {}, terminal() {}, error() {} }, restored ? [restored] : []);
  assert.equal(runtime.hasLive(), false);
  assert.equal(runtime.stop(j.id).state, "unknown");
});

test("admission persists before dispatch, returns immediately, ignores later caller abort, limits four active jobs", { skip: process.platform !== "darwin", timeout: 15_000 }, async () => {
  const dir = mkdtempSync(join(tmpdir(), "dots-runtime-test-"));
  const persisted: Job[] = [];
  const terminals: Job[] = [];
  const errors: string[] = [];
  const runtime = new JobsRuntime({ root: dir, persist: (j) => persisted.push(j), terminal: (j) => terminals.push(j), error: (e) => errors.push(e) });
  try {
    const controller = new AbortController();
    const first = runtime.start("sleep 1; exit 4", dir, 5000, controller.signal);
    assert.equal(first.state, "accepted");
    assert.equal(persisted[0]?.state, "accepted");
    assert.equal(terminals.length, 0);
    controller.abort();
    for (let i = 0; i < 3; i++) runtime.start("sleep 1; exit 0", dir, 5000);
    assert.throws(() => runtime.start("true", dir), /four/);
    assert.throws(() => runtime.start("true", dir, 0), /timeout/);
    assert.throws(() => runtime.start("true", dir, 1000, AbortSignal.abort()), /aborted/);
    await until(() => terminals.length === 4);
    assert.equal(runtime.get(first.id).facts.root?.code, 4);
    assert.equal(runtime.get(first.id).stopIntent, null);
    assert.equal(runtime.stop(first.id).state, "finished");
    assert.deepEqual(errors, []);
  } finally { await runtime.shutdown(); rmSync(dir, { recursive: true, force: true }); }
});

test("shutdown immediately after admission is bounded, sends no terminal record, persists facts", { skip: process.platform !== "darwin", timeout: 10_000 }, async () => {
  const dir = mkdtempSync(join(tmpdir(), "dots-shutdown-test-"));
  const saved: Job[] = [];
  const terminals: Job[] = [];
  const runtime = new JobsRuntime({ root: dir, persist: (j) => saved.push(j), terminal: (j) => terminals.push(j), error() {} });
  try {
    const j = runtime.start("sleep 20", dir);
    await runtime.shutdown();
    await runtime.shutdown();
    assert.equal(terminals.length, 0);
    assert.equal(saved.at(-1)?.stopIntent, "owner-shutdown");
    assert.ok(["finished", "unknown"].includes(runtime.get(j.id).state));
    assert.throws(() => runtime.start("true", dir), /closed/);
  } finally { await runtime.shutdown(); rmSync(dir, { recursive: true, force: true }); }
});

test("failed admission persistence never dispatches and releases reservation", { skip: process.platform !== "darwin" }, async () => {
  const dir = mkdtempSync(join(tmpdir(), "dots-admission-test-"));
  const runtime = new JobsRuntime({ root: dir, persist() { throw new Error("disk fixture failure"); }, terminal() {}, error() {} });
  try {
    assert.throws(() => runtime.start("true", dir), /disk fixture failure/);
    assert.equal(runtime.hasLive(), false);
    assert.deepEqual(runtime.list(), []);
  } finally { await runtime.shutdown(); rmSync(dir, { recursive: true, force: true }); }
});

test("runtime artifact budget evicts only its own finalized directories", { skip: process.platform !== "darwin", timeout: 30_000 }, async () => {
  const dir = mkdtempSync(join(tmpdir(), "dots-budget-test-"));
  const terminals: Job[] = [];
  const runtime = new JobsRuntime({ root: dir, persist() {}, terminal: (j) => terminals.push(j), error() {} });
  try {
    const admitted: Job[] = [];
    for (let batch = 0; batch < 4; batch++) {
      for (let i = 0; i < 4; i++) admitted.push(runtime.start("exit 0", dir));
      await until(() => terminals.length === (batch + 1) * 4);
    }
    // Eviction follows admission order, not nondeterministic completion order.
    const first = admitted[0];
    assert.ok(first);
    assert.equal(runtime.get(first.id).state, "finished");
    assert.equal(runtime.get(first.id).observationLoss, null);
    assert.equal(existsSync(first.directory), true);
    runtime.start("exit 0", dir);
    assert.equal(existsSync(first.directory), false);
    const runtimeDir = readdirSync(dir)[0];
    assert.ok(runtimeDir);
    assert.equal(readdirSync(join(dir, runtimeDir)).length, 16);
    await until(() => terminals.length === 17);
    runtime.replaceHistory([]);
    assert.deepEqual(runtime.list(), []);
    assert.throws(() => runtime.start("true", dir), /artifact budget exhausted/);
  } finally { await runtime.shutdown(); rmSync(dir, { recursive: true, force: true }); }
});
