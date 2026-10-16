import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import register, { layers, loadIndex, memoryContext, memoryDir, memoryRoot, projectKey, section } from "../extensions/memory/index.ts";

function scratchRepo() {
  const root = mkdtempSync(path.join(tmpdir(), "dots-memory-"));
  const checkout = path.join(root, "checkout");
  mkdirSync(checkout);
  const git = (cwd, ...args) => execFileSync("git", args, {
    cwd, stdio: "ignore",
    env: { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1",
      GIT_AUTHOR_NAME: "t", GIT_AUTHOR_EMAIL: "t@example.com", GIT_COMMITTER_NAME: "t", GIT_COMMITTER_EMAIL: "t@example.com" },
  });
  git(checkout, "init", "-q", "-b", "main");
  writeFileSync(path.join(checkout, "README.md"), "x\n");
  git(checkout, "add", "README.md");
  git(checkout, "commit", "-q", "-m", "init");
  git(checkout, "worktree", "add", "-q", path.join(root, "linked"), "-b", "slice");
  return { root, checkout, linked: path.join(root, "linked"), sub: path.join(checkout, "nested") };
}

const today = new Date("2026-10-15T12:00:00Z");

test("worktrees and subdirectories share one memory directory outside the repo", () => {
  const repo = scratchRepo();
  mkdirSync(repo.sub);
  try {
    const env = { XDG_DATA_HOME: path.join(repo.root, "data") };
    const main = memoryDir(repo.checkout, env);
    assert.equal(memoryDir(repo.linked, env), main);
    assert.equal(memoryDir(repo.sub, env), main);
    assert.ok(main.startsWith(path.join(repo.root, "data", "dots/memory")));
    assert.ok(!main.startsWith(repo.checkout));
    assert.equal(memoryDir(repo.root, env), null, "outside a repository there is no project memory");
    assert.deepEqual(layers(repo.linked, env).map((layer) => layer.layer), ["global", "project"]);
    assert.deepEqual(layers(repo.root, env).map((layer) => layer.layer), ["global"]);
    assert.equal(memoryRoot({ HOME: "/home/u" }), "/home/u/.local/share/dots/memory");
    assert.equal(projectKey("/Users/u/dev/a b/repo"), "Users-u-dev-a-b-repo");
  } finally {
    rmSync(repo.root, { recursive: true, force: true });
  }
});

test("one block carries the date, a pointer per absent layer, each index once, and the conventions once", () => {
  const repo = scratchRepo();
  try {
    const env = { XDG_DATA_HOME: path.join(repo.root, "data") };
    const globalFile = path.join(repo.root, "data/dots/memory/_global/MEMORY.md");
    const projectFile = path.join(memoryDir(repo.checkout, env), "MEMORY.md");

    const fresh = memoryContext(repo.linked, env, today).content;
    assert.match(fresh, /Today is 2026-10-15/);
    assert.ok(fresh.includes(globalFile) && fresh.includes(projectFile), "both paths known before any write");
    assert.equal(fresh.match(/Not created yet/g).length, 2);
    assert.equal(fresh.match(/Memory conventions:/g).length, 1);
    assert.match(fresh, /Never store credentials/);

    const outside = memoryContext(repo.root, env, today).content;
    assert.ok(outside.includes(globalFile) && !outside.includes("Project memory"), "no project section outside a repository");

    mkdirSync(path.dirname(globalFile), { recursive: true });
    writeFileSync(globalFile, "# Global memory\n\n- 2026-10-01: global fact\n");
    mkdirSync(path.dirname(projectFile), { recursive: true });
    writeFileSync(projectFile, "# Project memory\n\n- 2026-10-02: project fact\n");
    const loaded = memoryContext(repo.linked, env, today).content;
    assert.match(loaded, /## Global memory[\s\S]*global fact[\s\S]*## Project memory[\s\S]*project fact/);
    assert.doesNotMatch(loaded, /Not created yet/);

    const handlers = {};
    register({ on: (name, fn) => { handlers[name] = fn; }, registerCommand: () => {} });
    const previous = process.env.XDG_DATA_HOME;
    process.env.XDG_DATA_HOME = env.XDG_DATA_HOME;
    try {
      const contextFiles = [];
      handlers.before_agent_start({ systemPromptOptions: { contextFiles } }, { cwd: repo.linked });
      handlers.before_agent_start({ systemPromptOptions: { contextFiles } }, { cwd: repo.linked });
      assert.equal(contextFiles.length, 1, "a single memory entry, added once");
    } finally {
      if (previous === undefined) delete process.env.XDG_DATA_HOME; else process.env.XDG_DATA_HOME = previous;
    }
  } finally {
    rmSync(repo.root, { recursive: true, force: true });
  }
});

test("index loads bounded with exact counts", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "dots-memory-index-"));
  try {
    assert.equal(loadIndex(dir), null);
    assert.match(section(dir, "project"), /Not created yet/);
    const body = Array.from({ length: 300 }, (_, i) => `- line ${i}`).join("\n") + "\n";
    writeFileSync(path.join(dir, "MEMORY.md"), body);
    const index = loadIndex(dir);
    assert.equal(index.truncated, true);
    assert.match(index.content, /line 149\n\n\[memory index truncated: 150 of 300 lines loaded/);
    assert.doesNotMatch(index.content, /line 150\n/);
    assert.match(section(dir, "project"), /Index: .*MEMORY\.md/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
