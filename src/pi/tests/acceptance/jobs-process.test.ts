import assert from "node:assert/strict";
import { fork, spawnSync, type ChildProcess } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import test from "node:test";
import { Writable } from "node:stream";
import { Capture } from "../../extensions/jobs/capture.ts";
import { CHUNK_BYTES, PENDING_BYTES, PREFIX_BYTES, TAIL_BYTES, emptyFacts, type OwnerMessage, type SupervisorFacts, type SupervisorMessage } from "../../extensions/jobs/protocol.ts";

const supervisor = fileURLToPath(new URL("../../extensions/jobs/supervisor.ts", import.meta.url));
const fixture = fileURLToPath(new URL("./jobs-fixture.mjs", import.meta.url));
const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;
const mac = { skip: process.platform !== "darwin", timeout: 15_000 };
async function until(predicate: () => boolean, timeout = 7000): Promise<void> {
  const end = Date.now() + timeout;
  while (!predicate()) { if (Date.now() > end) throw new Error("fixture deadline expired"); await delay(25); }
}
function alive(pid: number): boolean {
  const result = spawnSync("ps", ["-o", "stat=", "-p", String(pid)], { encoding: "utf8", timeout: 1000 });
  return result.status === 0 && !result.stdout.trim().startsWith("Z");
}
function members(directory: string): number[] { return readdirSync(directory).filter((p) => p.startsWith("member-")).map((p) => Number(p.slice(7))); }
async function harness(run: (h: { child: ChildProcess; dir: string; ready: () => boolean; facts: () => SupervisorFacts; exited: () => boolean; send: (message: OwnerMessage) => void }) => Promise<void>): Promise<void> {
  const dir = mkdtempSync(join(tmpdir(), "dots-jobs-test-"));
  const child = fork(supervisor, [], { detached: true, stdio: ["ignore", "ignore", "ignore", "ipc"], execArgv: ["--experimental-strip-types"] });
  let facts = emptyFacts();
  let ready = false;
  let exited = false;
  child.on("error", () => {});
  child.on("message", (raw: unknown) => { const m = raw as SupervisorMessage; if (m.type === "ready") ready = true; else facts = m.facts; });
  child.once("exit", () => { exited = true; });
  const send = (m: OwnerMessage) => { if (child.connected) child.send(m, () => {}); };
  try { await run({ child, dir, facts: () => facts, ready: () => ready, exited: () => exited, send }); }
  finally {
    // Test-only survivor guard. Production never signals a reaped group's number.
    if (!exited && child.pid) { try { process.kill(-child.pid, "SIGKILL"); } catch { /* exited */ } }
    for (const pid of members(dir)) if (alive(pid)) { try { process.kill(pid, "SIGKILL"); } catch { /* exited */ } }
    await until(() => exited).catch(() => {});
    rmSync(dir, { recursive: true, force: true });
  }
}

for (const mode of ["ignore", "trap", "chatty"]) test(`root exit cleans ${mode} descendant with fixed grace`, mac, async () => {
  await harness(async (h) => {
    await until(h.ready);
    const start = Date.now();
    h.send({ type: "start", command: `${quote(process.execPath)} ${quote(fixture)} ${mode} ${quote(h.dir)} & sleep 0.3; exit 3`, cwd: h.dir, directory: h.dir, timeoutMs: 10_000 });
    await until(h.exited);
    assert.ok(Date.now() - start >= 3000);
    assert.equal(h.facts().root?.code, 3);
    assert.equal(h.facts().reason, "root-exit");
    assert.equal(h.facts().termSent, true);
    assert.equal(h.facts().killIntent, true);
    assert.ok(members(h.dir).length > 0);
    await until(() => members(h.dir).every((pid) => !alive(pid)));
    if (mode === "trap") assert.equal(readFileSync(join(h.dir, "trapped"), "utf8"), "TERM");
  });
});

test("repeated stop preserves first intent and fixed deadline; ignored TERM outcome unobserved", mac, async () => {
  await harness(async (h) => {
    await until(h.ready);
    h.send({ type: "start", command: `exec ${quote(process.execPath)} ${quote(fixture)} ignore ${quote(h.dir)}`, cwd: h.dir, directory: h.dir, timeoutMs: 10_000 });
    await until(() => members(h.dir).length > 0);
    const start = Date.now();
    for (let i = 0; i < 3; i++) { h.send({ type: "stop", reason: "stop" }); await delay(200); }
    await until(h.exited);
    assert.ok(Date.now() - start < 4200);
    assert.equal(h.facts().reason, "stop");
    assert.equal(h.facts().root, null);
    await until(() => members(h.dir).every((pid) => !alive(pid)));
  });
});

test("timeout runs TERM trap and capture final flush", mac, async () => {
  await harness(async (h) => {
    await until(h.ready);
    h.send({ type: "start", command: `exec ${quote(process.execPath)} ${quote(fixture)} trap ${quote(h.dir)}`, cwd: h.dir, directory: h.dir, timeoutMs: 400 });
    await until(h.exited);
    assert.equal(h.facts().reason, "timeout");
    assert.equal(h.facts().root?.code, 0);
    assert.equal(h.facts().capture.prefixFlushed, true);
    assert.equal(h.facts().capture.tailFlushed, true);
  });
});

test("cap drains healthy payload to completion; prefix/tail and queue stay bounded", mac, async () => {
  await harness(async (h) => {
    await until(h.ready);
    h.send({ type: "start", command: `exec ${quote(process.execPath)} ${quote(fixture)} volume ${quote(h.dir)}`, cwd: h.dir, directory: h.dir, timeoutMs: 10_000 });
    await until(h.exited);
    const c = h.facts().capture;
    assert.equal(h.facts().root?.code, 0);
    assert.ok(c.bytesSeen > PREFIX_BYTES);
    assert.equal(c.capped, true);
    assert.equal(c.droppedBytes, 0);
    assert.equal(statSync(join(h.dir, "output.log")).size, PREFIX_BYTES);
    assert.equal(statSync(join(h.dir, "tail.log")).size, TAIL_BYTES);
    assert.match(readFileSync(join(h.dir, "tail.log"), "utf8"), /CAP_DRAIN_FINISHED/);
    assert.ok(c.peakPendingBytes <= PENDING_BYTES + CHUNK_BYTES);
    assert.equal(statSync(h.dir).mode & 0o077, 0);
    assert.equal(statSync(join(h.dir, "output.log")).mode & 0o077, 0);
  });
});

test("disk errors preserve draining and do not claim a complete capture", mac, async () => {
  await harness(async (h) => {
    mkdirSync(join(h.dir, "output.log")); // neutral, deterministic EEXIST (no privileged disk setup)
    await until(h.ready);
    h.send({ type: "start", command: `exec ${quote(process.execPath)} ${quote(fixture)} volume ${quote(h.dir)}`, cwd: h.dir, directory: h.dir, timeoutMs: 10_000 });
    await until(h.exited);
    assert.equal(h.facts().root?.code, 0);
    assert.match(h.facts().capture.error ?? "", /EEXIST/);
    assert.equal(h.facts().capture.prefixFlushed, false);
    assert.equal(h.facts().capture.capped, true);
    assert.ok(h.facts().capture.droppedBytes > 0);
  });
});

test("disconnect before start never launches, disconnect after launch cleans", mac, async () => {
  for (const start of [false, true]) await harness(async (h) => {
    await until(h.ready);
    if (start) {
      h.send({ type: "start", command: `exec ${quote(process.execPath)} ${quote(fixture)} ignore ${quote(h.dir)}`, cwd: h.dir, directory: h.dir, timeoutMs: 10_000 });
      await until(() => members(h.dir).length > 0);
    }
    h.child.disconnect();
    await until(h.exited);
    await until(() => members(h.dir).every((pid) => !alive(pid)));
    if (!start) assert.deepEqual(members(h.dir), []);
  });
});

test("unexpected supervisor SIGKILL is not process containment", mac, async () => {
  await harness(async (h) => {
    await until(h.ready);
    h.send({ type: "start", command: `exec ${quote(process.execPath)} ${quote(fixture)} ignore ${quote(h.dir)}`, cwd: h.dir, directory: h.dir, timeoutMs: 10_000 });
    await until(() => members(h.dir).length > 0);
    h.child.kill("SIGKILL");
    await until(h.exited);
    assert.equal(h.facts().killIntent, false);
    assert.ok(members(h.dir).some(alive)); // Test guard kills these, not the production parent.
  });
});

test("parent SIGKILL before ready/no start and during start races leaves no survivors", { ...mac, timeout: 30_000 }, async () => {
  for (const timing of ["before-ready", "no-start", "0", "2", "7", "300"]) {
    const dir = mkdtempSync(join(tmpdir(), "dots-owner-test-"));
    const command = `exec ${quote(process.execPath)} ${quote(fixture)} ignore ${quote(dir)}`;
    const owner = fork(fileURLToPath(new URL("./jobs-owner-fixture.mjs", import.meta.url)), [supervisor, dir, command, timing], { stdio: ["ignore", "ignore", "ignore", "ipc"] });
    let supervisorPid: number | undefined;
    let exited = false;
    owner.on("message", (m: { supervisor: number }) => { supervisorPid = m.supervisor; });
    owner.on("exit", () => { exited = true; });
    try {
      await until(() => exited && supervisorPid !== undefined);
      await until(() => supervisorPid !== undefined && !alive(supervisorPid));
      await until(() => members(dir).every((pid) => !alive(pid)));
    } finally {
      if (!exited) owner.kill("SIGKILL");
      if (supervisorPid && alive(supervisorPid)) { try { process.kill(-supervisorPid, "SIGKILL"); } catch { /* exited */ } }
      for (const pid of members(dir)) if (alive(pid)) { try { process.kill(pid, "SIGKILL"); } catch { /* exited */ } }
      rmSync(dir, { recursive: true, force: true });
    }
  }
});

test("stalled writer cannot extend final flush deadline", { timeout: 3000 }, async () => {
  const dir = mkdtempSync(join(tmpdir(), "dots-stall-test-"));
  const stalled = new Writable({ write() { /* Deliberately never completes. */ } });
  try {
    const capture = new Capture(dir, () => {}, stalled);
    capture.data(Buffer.alloc(PREFIX_BYTES));
    await capture.finish(50);
    assert.equal(capture.facts.flushExpired, true);
    assert.equal(capture.facts.prefixFlushed, false);
    assert.ok(capture.facts.peakPendingBytes <= PENDING_BYTES + CHUNK_BYTES);
  } finally { stalled.destroy(); rmSync(dir, { recursive: true, force: true }); }
});

test("capture rejects excess pending chunks without unbounded allocation", async () => {
  const dir = mkdtempSync(join(tmpdir(), "dots-capture-test-"));
  try {
    const capture = new Capture(dir, () => {});
    capture.data(Buffer.alloc(PREFIX_BYTES + 100));
    await capture.finish();
    assert.ok(capture.facts.peakPendingBytes <= PENDING_BYTES + CHUNK_BYTES);
    assert.ok(capture.facts.droppedBytes > 0);
    assert.equal(capture.facts.tailBytes, TAIL_BYTES);
    assert.equal(capture.facts.capped, true);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
