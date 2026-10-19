import assert from "node:assert/strict";
import test from "node:test";
import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";
import { runCheck, checkDigest } from "../../extensions/wake/check.ts";

test("check capture is bounded but compares complete stdout and stderr", async () => {
  const command = (suffix) => `${JSON.stringify(process.execPath)} -e 'process.stdout.write("x".repeat(100000) + "${suffix}"); process.stderr.write("diagnostic")'`;
  const a = await runCheck(command("a"), process.cwd());
  const b = await runCheck(command("b"), process.cwd());
  assert.equal(a.code, 0);
  assert.equal(a.stdout.length, 4000);
  assert.equal(a.stderr, "diagnostic");
  assert.equal(a.truncated, true);
  assert.equal(a.stdout, b.stdout);
  assert.notEqual(a.digest, b.digest, "changes beyond the preview still wake the session");
  assert.equal(a.digest, checkDigest("x".repeat(100000) + "a", "diagnostic"));
});

for (const exitedBeforeDeadline of [true, false]) {
  test(`retained pipes are bounded without signaling a reaped leader (exited first: ${exitedBeforeDeadline})`, { skip: process.platform === "win32" }, async (t) => {
    // Node reports leader exit before close when a descendant retains pipes.
    // Drive that lifecycle explicitly; no real orphan, startup race, or PID signal.
    const child = Object.assign(new EventEmitter(), {
      pid: 12345, exitCode: null, signalCode: null,
      stdout: new PassThrough(), stderr: new PassThrough(),
    });
    const signals = [];
    t.mock.method(process, "kill", (pid, signal) => { signals.push([pid, signal]); return true; });
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const pending = runCheck("fixture", process.cwd(), undefined, 100, () => child);
    child.stdout.write("ready");
    const exit = () => { child.exitCode = 0; child.emit("exit", 0, null); };
    if (exitedBeforeDeadline) exit();
    t.mock.timers.tick(100);
    if (!exitedBeforeDeadline) exit();
    t.mock.timers.tick(1000);
    t.mock.timers.tick(250);
    const result = await pending;
    assert.equal(result.stdout, "ready");
    assert.equal(result.code, -1);
    assert.match(result.stderr, /output drain incomplete/);
    assert.equal(child.stdout.destroyed && child.stderr.destroyed, true);
    assert.deepEqual(signals, exitedBeforeDeadline ? [] : [[-12345, "SIGTERM"]]);
    child.emit("close", 0, null);
    t.mock.timers.tick(10_000);
    assert.deepEqual(signals, exitedBeforeDeadline ? [] : [[-12345, "SIGTERM"]]);
  });
}

test("timeout, signal termination, cancellation, and spawn errors never count as success", { timeout: 5000 }, async () => {
  const timed = await runCheck("sleep 10", process.cwd(), undefined, 30);
  assert.equal(timed.code, -1);
  assert.match(timed.stderr, /timed out/);
  const killed = await runCheck("kill -TERM $$", process.cwd());
  assert.equal(killed.code, -1);
  assert.match(killed.stderr, /terminated/);
  const controller = new AbortController();
  const pending = runCheck("sleep 10", process.cwd(), controller.signal);
  controller.abort();
  assert.match((await pending).stderr, /cancelled/);
  assert.equal((await runCheck("true", process.cwd(), controller.signal)).code, -1);
  assert.equal((await runCheck("true", "/nonexistent-dots-test-directory")).code, -1);
});
