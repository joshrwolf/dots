import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { BRIEF_BYTES, decodeList, decodeSnapshot, readBrief } from "../extensions/goal/source.ts";
import { snapshot } from "./goal-fixture.mjs";

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
