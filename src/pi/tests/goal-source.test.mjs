import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { BRIEF_BYTES, decodeList, decodeSnapshot, readBrief, dotsSource, withHierarchy } from "../extensions/goal/source.ts";
import { snapshot, task } from "./goal-fixture.mjs";

test("CLI responses are validated rather than coerced into success", () => {
  assert.deepEqual(decodeSnapshot(snapshot()), snapshot());
  assert.equal(decodeList({ project: "example", goals: [] }).goals.length, 0);
  for (const value of [null, {}, { error: "unsupported" }, { ...snapshot(), overview: {} }]) assert.throws(() => decodeSnapshot(value));
  for (const changed of ["status", "progress", "brief"]) {
    const value = snapshot();
    if (changed === "status") value.overview.goal.root.status = "unknown";
    if (changed === "progress") value.overview.progress.done = -1;
    if (changed === "brief") value.overview.goal.brief = "relative.md";
    assert.throws(() => decodeSnapshot(value));
  }
});

test("source reads complete history once while keeping closed details out of ordinary context", async () => {
  const dir = mkdtempSync(join(tmpdir(), "dots-goal-source-"));
  const oldPath = process.env.PATH;
  const full = snapshot();
  full.overview.goal.root.children = 2;
  full.overview.tasks.push(task("t-999999", "Closed fixture slice", { parent: "t-123abc", status: "done" }));
  full.overview.progress.total = 2; full.overview.progress.done = 1;
  const argsFile = join(dir, "args.json");
  writeFileSync(join(dir, "dots"), `#!/usr/bin/env node\nrequire('node:fs').writeFileSync(${JSON.stringify(argsFile)}, JSON.stringify(process.argv.slice(2)));\nprocess.stdout.write(${JSON.stringify(JSON.stringify(full))});\n`, { mode: 0o700 });
  process.env.PATH = `${dir}:${oldPath}`;
  try {
    const result = await dotsSource.show(dir, "t-123abc", new AbortController().signal);
    assert.deepEqual(JSON.parse(readFileSync(argsFile, "utf8")), ["--directory", dir, "--json", "task", "goal", "show", "t-123abc", "--all"]);
    assert.equal(result.overview.tasks.length, 1);
    assert.equal(result.overview.hidden_closed, 1);
    assert.deepEqual(result.allTasks, full.overview.tasks);
    assert.deepEqual(result.overview.progress, full.overview.progress);
    const history = await dotsSource.show(dir, "t-123abc", new AbortController().signal, true);
    assert.equal(history.overview.tasks.length, 2);
    assert.equal(history.overview.hidden_closed, 0);
    assert.deepEqual(withHierarchy(result, true), history);
    const partial = { ...full, overview: { ...full.overview, hidden_closed: 1 } };
    assert.equal(withHierarchy(partial), partial);
  } finally {
    if (oldPath === undefined) delete process.env.PATH; else process.env.PATH = oldPath;
    rmSync(dir, { recursive: true, force: true });
  }
});

test("brief loading is bounded and reports missing files, directories, and FIFOs", async () => {
  const dir = mkdtempSync(join(tmpdir(), "dots-goal-brief-"));
  try {
    const file = join(dir, "brief.md");
    writeFileSync(file, "# café 👩‍💻\nAccepted pattern");
    assert.deepEqual(await readBrief(file), { text: "# café 👩‍💻\nAccepted pattern" });
    writeFileSync(file, "x".repeat(BRIEF_BYTES - 1) + "👩‍💻".repeat(100));
    const bounded = await readBrief(file);
    assert.match(bounded.warning, /truncated/);
    assert.equal(bounded.text.length, BRIEF_BYTES - 1);
    assert.doesNotMatch(bounded.text, /\ufffd/);
    assert.match((await readBrief(join(dir, "missing"))).warning, /unavailable/);
    assert.match((await readBrief(dir)).warning, /not a regular file/);
    if (process.platform !== "win32") {
      const fifo = join(dir, "fifo"); execFileSync("mkfifo", [fifo]);
      assert.match((await readBrief(fifo)).warning, /not a regular file/);
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
