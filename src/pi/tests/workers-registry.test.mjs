import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { openRegistry } from "../extensions/workers/registry.ts";
import { fleetStatus } from "../extensions/workers/present.ts";

const row = (id, parent, root, state, extra = {}) => ({
  id, parent, root, owner_session: null, label: id, state, cwd: "/w", model: null, activity: null, failed: null, question: null,
  calls: 0, tokens: 0, started_at: 1000, last_event_at: 1000, updated_at: 1000, session_file: null, pid: 1, ...extra,
});

test("registry: tree summary counts descendants at any depth by state, and silences the sqlite warning", async () => {
  const dir = mkdtempSync(path.join(tmpdir(), "dots-registry-"));
  const warnings = [];
  const onWarning = (w) => warnings.push(String(w.message ?? w));
  process.on("warning", onWarning);
  try {
    const reg = await openRegistry(path.join(dir, "nested/workers.db"), (m) => assert.fail(m));
    assert.ok(reg);
    reg.upsert(row("a", null, "a", "running"));
    reg.upsert(row("a1", "a", "a", "running", { last_event_at: 0 }));
    reg.upsert(row("a2", "a", "a", "asking", { question: "which?" }));
    reg.upsert(row("a2x", "a2", "a", "running", { failed: "bash", last_event_at: 95_000 }));
    reg.upsert(row("a3", "a", "a", "completed"));
    reg.upsert(row("b", null, "b", "running"));

    assert.deepEqual(reg.subtree("a", 100_000, 90_000), { live: 3, asking: 1, failed: 1, quiet: 1 });
    assert.deepEqual(reg.subtree("a2", 100_000, 90_000), { live: 1, asking: 0, failed: 1, quiet: 0 });
    assert.deepEqual(reg.subtree("b", 100_000, 90_000), { live: 0, asking: 0, failed: 0, quiet: 0 });
    assert.deepEqual(reg.children(null).map((r) => r.id), ["a", "b"]);

    reg.upsert(row("a1", "a", "a", "completed"));
    assert.equal(reg.subtree("a", 100_000, 90_000).live, 2, "upsert replaces, never duplicates");

    // A hard-killed owner never marks its rows; a dead pid makes them lost, not live.
    reg.upsert(row("orphan", "a", "a", "running", { pid: 999_999 }));
    assert.equal(reg.subtree("a", 100_000, 90_000, (pid) => pid !== 999_999).live, 2);
    assert.equal(reg.get("orphan").state, "lost");
    assert.equal(reg.children("a", () => true).find((r) => r.id === "a2").state, "asking", "live owners untouched");

    reg.event({ worker: "a1", at: 1, kind: "call", text: "bash ls" });
    reg.event({ worker: "a1", at: 2, kind: "said", text: "x".repeat(5000) });
    const events = reg.events("a1", 10);
    assert.equal(events.length, 2);
    assert.equal(events[0].kind, "said", "most recent first");
    assert.equal(events[0].text.length, 2000, "event text is bounded");
    for (let at = 0; at < 250; at++) reg.event({ worker: "bounded", at, kind: "note", text: "event" });
    assert.equal(reg.events("bounded", 1000).length, 200);
    assert.equal(reg.events("bounded", 1000).at(-1).at, 50);
    reg.upsert(row("cycle1", "cycle2", "cycle1", "running"));
    reg.upsert(row("cycle2", "cycle1", "cycle1", "running"));
    assert.ok(reg.subtree("cycle1", 100_000, 90_000).live <= 2, "corrupt cycles terminate");
    reg.close();
    reg.close();
  } finally {
    process.off("warning", onWarning);
    rmSync(dir, { recursive: true, force: true });
  }
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(warnings.filter((w) => /SQLite/.test(w)), [], "no ExperimentalWarning reaches the terminal");
});

test("fleet status folds descendants into a few characters", () => {
  assert.equal(fleetStatus([]), "");
  const r = (state, extra = {}) => ({ label: "x", state, depth: 0, elapsed: "", tokens: 0, calls: 0, detail: "", idleMs: 0, ...extra });
  assert.equal(fleetStatus([r("running")]), "workers 1");
  assert.equal(fleetStatus([r("completed", { idleMs: 1_000_000 }), r("asking", { depth: 1 })]), "workers 1 · ?1");
  assert.equal(
    fleetStatus([r("running"), r("asking", { depth: 1 }), r("running", { depth: 1, idleMs: 10 * 60_000 }), r("asking", { failed: "bash" })]),
    "workers 4 \u00b7 ?2 \u00b7 \u27171 \u00b7 quiet 1",
  );
});
