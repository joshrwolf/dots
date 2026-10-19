import assert from "node:assert/strict";
import test from "node:test";
import { runCheck, checkDigest } from "../extensions/wake/check.ts";

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

test("escaped descendants retaining pipes cannot hold check completion indefinitely", { timeout: 5000, skip: process.platform === "win32" }, async () => {
  const script = 'const {spawn}=require("node:child_process"); const c=spawn(process.execPath,["-e","setTimeout(()=>process.exit(),4000)"],{detached:true,stdio:["ignore","inherit","inherit"]}); console.log(c.pid); c.unref();';
  const result = await runCheck(`${JSON.stringify(process.execPath)} -e '${script}'`, process.cwd(), undefined, 500);
  const pid = Number(result.stdout.trim());
  try {
    assert.ok(Number.isInteger(pid) && pid > 1, "fixture published its escaped child pid");
    assert.equal(result.code, -1);
    assert.match(result.stderr, /output drain incomplete/);
  } finally { if (Number.isInteger(pid) && pid > 1) { try { process.kill(-pid, "SIGKILL"); } catch {} } }
});

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
