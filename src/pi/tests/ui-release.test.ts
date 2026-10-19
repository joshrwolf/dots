import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, renameSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { releaseDescription, releaseTracker } from "../src/host/release.ts";

test("loaded identity stays fixed while installed identity follows switches and rollback", () => {
  const directory = mkdtempSync(join(tmpdir(), "dots-release-status-"));
  const a = "a".repeat(64), b = "b".repeat(64);
  const current = join(directory, "current");
  const switchTo = (id: string) => { symlinkSync(join(directory, id), current + ".next"); renameSync(current + ".next", current); };
  try {
    mkdirSync(join(directory, a)); mkdirSync(join(directory, b));
    switchTo(a);
    const oldSession = releaseTracker(current, current);
    assert.deepEqual(oldSession(), { loaded: a, installed: a, reloadNeeded: false });
    assert.match(releaseDescription(oldSession()), /up to date/);
    switchTo(b);
    assert.deepEqual(oldSession(), { loaded: a, installed: b, reloadNeeded: true });
    assert.match(releaseDescription(oldSession()), /loaded aaaaaaaaaaaa · installed bbbbbbbbbbbb · \/reload when background work is idle/);
    const newSession = releaseTracker(current, current);
    assert.equal(newSession().reloadNeeded, false);
    switchTo(a);
    assert.equal(oldSession().reloadNeeded, false);
    assert.equal(newSession().reloadNeeded, true);
    rmSync(current);
    assert.equal(newSession().installed, undefined);
    assert.doesNotMatch(releaseDescription(newSession()), /up to date/);
    const development = releaseTracker(directory, current);
    assert.equal(development().loaded, "checkout");
    assert.equal(development().reloadNeeded, false);
    assert.match(releaseDescription(development()), /development source/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("the normal install task verifies first; checks and verification never install", () => {
  const root = fileURLToPath(new URL("../../../", import.meta.url));
  const info = (name: string) => {
    const result = spawnSync("mise", ["tasks", "info", `//src/pi:${name}`, "--json"], { cwd: root, encoding: "utf8", timeout: 30_000 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout) as { depends: string[]; run: string[]; cache: { enabled: boolean } };
  };
  const install = info("install");
  assert.deepEqual(install.depends, [":verify"]);
  assert.deepEqual(install.run, ["node install/index.mjs"]);
  assert.equal(install.cache.enabled, false);
  for (const name of ["check", "verify", "prepare", "package"]) {
    const task = info(name);
    assert.ok(task.depends.every((dependency) => !dependency.endsWith(":install")));
    assert.ok(task.run.every((command) => !command.includes("node install/index.mjs")));
  }
});
