import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { CHILD_CONTRACT, loadConfig, resolveModel, splitThinking } from "../extensions/workers/config.ts";
import { PiProcess } from "../extensions/workers/rpc.ts";

// A stand-in for `pi --mode rpc`: answers commands, emits a question and a
// settled run, and leaves a U+2028 inside a string to prove LF-only framing.
const FAKE_PI = `
let buffer = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  buffer += chunk;
  let i;
  while ((i = buffer.indexOf("\\n")) !== -1) {
    const line = buffer.slice(0, i); buffer = buffer.slice(i + 1);
    const cmd = JSON.parse(line);
    const out = (o) => process.stdout.write(JSON.stringify(o) + "\\n");
    if (cmd.type === "prompt") {
      out({ id: cmd.id, type: "response", command: "prompt", success: true, data: { disposition: "started" } });
      out({ type: "tool_execution_start", toolCallId: "t1", toolName: "bash", args: {} });
      out({ type: "message_update", usage: { totalTokens: 42 }, assistantMessageEvent: { type: "text_delta", delta: "para\\u2028graph" } });
      out({ type: "extension_ui_request", id: "ui-1", method: "input", title: "Which base?" });
    } else if (cmd.type === "extension_ui_response") {
      global.answer = cmd.value;
      out({ type: "tool_execution_end", toolCallId: "t1", toolName: "bash" });
      out({ type: "agent_settled", aborted: false });
    } else if (cmd.type === "get_last_assistant_text") {
      out({ id: cmd.id, type: "response", command: cmd.type, success: true, data: { text: "done with " + global.answer } });
    } else if (cmd.type === "get_state") {
      out({ id: cmd.id, type: "response", command: cmd.type, success: true, data: { sessionFile: "/tmp/fake.jsonl" } });
    } else if (cmd.type === "set_model") {
      out({ id: cmd.id, type: "response", command: cmd.type, success: false, error: "Model not found" });
    } else {
      out({ id: cmd.id, type: "response", command: cmd.type, success: true, data: {} });
    }
  }
});
process.stdin.on("end", () => process.exit(0));
process.stderr.write("fake pi started\\n");
`;

function fakePi() {
  const dir = mkdtempSync(path.join(tmpdir(), "dots-workers-"));
  const cli = path.join(dir, "cli.js");
  writeFileSync(cli, FAKE_PI);
  return { dir, cli };
}

test("protocol: correlation, LF-only framing, ui request/response, settle, orderly close", async () => {
  const { dir, cli } = fakePi();
  const child = new PiProcess({ cwd: dir, args: ["--no-session"], command: [process.execPath, cli] });
  try {
    const records = [];
    child.onRecord((record) => records.push(record));
    const prompt = await child.call({ type: "prompt", message: "go" });
    assert.equal(prompt.disposition, "started");
    await new Promise((resolve) => setTimeout(resolve, 50));
    const ask = records.find((record) => record.type === "extension_ui_request");
    assert.equal(ask.id, "ui-1");
    assert.ok(records.some((record) => record.type === "message_update" && record.assistantMessageEvent.delta === "para\u2028graph"));
    child.respondUi("ui-1", { value: "main" });
    await new Promise((resolve) => setTimeout(resolve, 50));
    assert.ok(records.some((record) => record.type === "agent_settled"));
    const text = await child.call({ type: "get_last_assistant_text" });
    assert.equal(text.text, "done with main");
    await assert.rejects(child.call({ type: "set_model" }), /Model not found/);
    assert.match(child.stderr.join(""), /fake pi started/);
    assert.equal(child.alive, true);
    await child.close(1000);
    assert.equal(child.alive, false);
    await assert.rejects(child.call({ type: "get_state" }), /not running/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("models resolve through modelRoles and thinking suffixes; the child contract is explicit", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "dots-workers-config-"));
  try {
    const file = path.join(dir, "workers.json");
    writeFileSync(file, JSON.stringify({ modelRoles: { review: "anthropic/claude-opus-5-5:high", worker: "anthropic/claude-sonnet-5-5" } }));
    const config = loadConfig(file);
    assert.deepEqual(resolveModel("@review", config), { model: "anthropic/claude-opus-5-5", thinking: "high" });
    assert.deepEqual(resolveModel("@worker", config), { model: "anthropic/claude-sonnet-5-5", thinking: undefined });
    assert.deepEqual(resolveModel("openai-codex/gpt-6-astra:low", config), { model: "openai-codex/gpt-6-astra", thinking: "low" });
    assert.deepEqual(resolveModel(undefined, config), {});
    assert.match(resolveModel("@missing", config).error, /modelRoles/);
    assert.deepEqual(splitThinking("provider/model:with:colons"), ["provider/model:with:colons", undefined]);
    assert.deepEqual(loadConfig(path.join(dir, "absent.json")), { modelRoles: {}, maxDepth: 3 });
    assert.match(CHILD_CONTRACT, /final message is your complete result/);
    assert.match(CHILD_CONTRACT, /message \{ to: "parent"/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
