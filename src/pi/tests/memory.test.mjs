import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
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
    const alias = path.join(repo.root, "alias");
    symlinkSync(repo.checkout, alias);
    assert.equal(memoryDir(alias, env), main);
    assert.equal(main, path.join(env.XDG_DATA_HOME, "dots/memory", projectKey(realpathSync(repo.checkout))));
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
      writeFileSync(projectFile, "# Project memory\n\n- 2026-10-15: corrected fact; see wiki/decision.md\n");
      handlers.before_agent_start({ systemPromptOptions: { contextFiles } }, { cwd: repo.linked });
      assert.equal(contextFiles.length, 1);
      assert.match(contextFiles[0].content, /corrected fact/);
      assert.doesNotMatch(contextFiles[0].content, /2026-10-02: project fact/);
      assert.match(contextFiles[0].content, /cooperative .write-lock/);
    } finally {
      if (previous === undefined) delete process.env.XDG_DATA_HOME; else process.env.XDG_DATA_HOME = previous;
    }
  } finally {
    rmSync(repo.root, { recursive: true, force: true });
  }
});

test("only indexes load automatically; evidence and unrelated projects stay out", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "dots-memory-scope-"));
  try {
    const global = path.join(dir, "dots/memory/_global");
    mkdirSync(path.join(global, "wiki"), { recursive: true });
    mkdirSync(path.join(global, "sources"));
    writeFileSync(path.join(global, "MEMORY.md"), "- 2026-10-15: [decision](wiki/decision.md)\n");
    writeFileSync(path.join(global, "wiki/decision.md"), "DETAIL_ONLY");
    writeFileSync(path.join(global, "sources/check.md"), "SOURCE_ONLY");
    mkdirSync(path.join(dir, "dots/memory/unrelated"));
    writeFileSync(path.join(dir, "dots/memory/unrelated/MEMORY.md"), "OTHER_PROJECT_ONLY");
    const content = memoryContext(dir, { XDG_DATA_HOME: dir }, today).content;
    assert.match(content, /wiki\/decision.md/);
    assert.doesNotMatch(content, /DETAIL_ONLY|SOURCE_ONLY|OTHER_PROJECT_ONLY/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("index limit counts UTF-8 bytes rather than UTF-16 characters", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "dots-memory-unicode-"));
  try {
    const line = "界".repeat(300);
    writeFileSync(path.join(dir, "MEMORY.md"), Array(40).fill(line).join("\n"));
    const index = loadIndex(dir);
    assert.equal(index.truncated, true);
    const body = index.content.split("\n\n[memory index truncated:")[0];
    assert.ok(Buffer.byteLength(body, "utf8") <= 24 * 1024);
    assert.equal(body.split("\n").length, 27);
  } finally { rmSync(dir, { recursive: true, force: true }); }
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
