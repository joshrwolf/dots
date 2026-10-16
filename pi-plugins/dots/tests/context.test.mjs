import assert from "node:assert/strict";
import test from "node:test";
import { formatSkillsForPrompt } from "@earendil-works/pi-coding-agent";
import register, { contextReport } from "../extensions/context/index.ts";

const skill = {
  name: "example", description: "Example & guidance", filePath: "/skills/example/SKILL.md",
  baseDir: "/skills/example", source: "user", disableModelInvocation: false,
};
function fixture(overrides = {}) {
  const pi = {
    getActiveTools: () => ["read"],
    getAllTools: () => [
      { name: "read", description: "Read files", parameters: { type: "object" } },
      { name: "inactive-tool", description: "Not loaded", parameters: {} },
    ],
  };
  const ctx = {
    getContextUsage: () => ({ tokens: 10000, contextWindow: 200000, percent: 5 }),
    getSystemPrompt: () => `Base\nFile guidance\n${formatSkillsForPrompt([skill])}`,
    getSystemPromptOptions: () => ({ cwd: "/project", skills: [skill], contextFiles: [
      { path: "/project/AGENTS.md", content: "File guidance" },
    ] }),
    isIdle: () => true,
    sessionManager: { buildSessionProjection: () => ({ messages: [
      { role: "user", content: "Private prompt content", timestamp: 0 },
      { role: "toolResult", toolName: "read", toolCallId: "one", content: [{ type: "text", text: "Private file content" }], isError: false, timestamp: 0 },
    ] }) },
    ...overrides,
  };
  return { pi, ctx };
}

test("shows loaded source labels and active tools without content or scaled estimates", () => {
  const { pi, ctx } = fixture();
  const report = contextReport(pi, ctx);
  assert.match(report, /10,000 \/ 200,000/);
  assert.match(report, /Instruction files \(1\)/);
  assert.match(report, /Skill catalog entries \(1\)/);
  assert.match(report, /AGENTS\.md/);
  assert.match(report, /example/);
  assert.match(report, /Tool results: read/);
  assert.doesNotMatch(report, /Private|File guidance|Example &|inactive-tool/);
  assert.match(report, /~\s+4\s+0\.00%\s+└─ \/project\/AGENTS.md/);
  assert.match(report, /tokens \(5\.00%\)/);
});

test("codemode omits misleading raw schema estimates in on and only modes", () => {
  for (const active of [["codemode", "read"], ["codemode"]]) {
    const { pi, ctx } = fixture();
    pi.getActiveTools = () => active;
    pi.getAllTools = () => { throw new Error("Raw schemas are not the request loadout"); };
    const report = contextReport(pi, ctx);
    assert.match(report, /Estimate unavailable: codemode rewrites tool declarations/);
    assert.doesNotMatch(report, /Registered tool schemas/);
    assert.match(report, /Conversation/);
  }
});

test("inactive codemode does not suppress ordinary schema estimates", () => {
  const { pi, ctx } = fixture();
  pi.getAllTools = () => [
    { name: "read", description: "Read files", parameters: {} },
    { name: "codemode", description: "Inactive", parameters: {} },
  ];
  const report = contextReport(pi, ctx);
  assert.match(report, /Registered tool schemas \(1\)/);
  assert.doesNotMatch(report, /Estimate unavailable|Inactive/);
});

test("percentages use model capacity, including when total usage is unknown", () => {
  const { pi, ctx } = fixture({
    getContextUsage: () => undefined,
    model: { contextWindow: 1000 },
    getSystemPrompt: () => "x".repeat(400),
  });
  assert.match(contextReport(pi, ctx), /~\s+100\s+10\.00%  System prompt/);
  ctx.model = undefined;
  assert.match(contextReport(pi, ctx), /~\s+100\s+n\/a  System prompt/);
});

test("unknown usage is not zero; absent resources in forced prompts are not attributed", () => {
  const { pi, ctx } = fixture({
    getContextUsage: () => ({ tokens: null, contextWindow: 200000, percent: null }),
    getSystemPrompt: () => "Replacement",
    isIdle: () => false,
  });
  const report = contextReport(pi, ctx);
  assert.match(report, /usage: unknown/);
  assert.match(report, /Agent is active/);
  assert.match(report, /Instruction files \(0\)/);
  assert.match(report, /Skill catalog entries \(0\)/);
});

test("hidden skills, absent files, and duplicate contents are not counted twice", () => {
  const { pi, ctx } = fixture({ getSystemPromptOptions: () => ({
    cwd: "/project", skills: [{ ...skill, disableModelInvocation: true }],
    contextFiles: [
      { path: "first", content: "File guidance" },
      { path: "second", content: "File guidance" },
      { path: "absent", content: "Not in prompt" },
    ],
  }) });
  const report = contextReport(pi, ctx);
  assert.match(report, /Instruction files \(1\)/);
  assert.match(report, /Skill catalog entries \(0\)/);
  assert.doesNotMatch(report, /second|absent/);
});

test("uses projected messages and excludes historical system messages from conversation", () => {
  const { pi, ctx } = fixture({ sessionManager: { buildSessionProjection: () => ({ messages: [
    { role: "system", content: "Old system", timestamp: 0 },
    { role: "user", content: "Kept", timestamp: 0 },
  ] }) } });
  const report = contextReport(pi, ctx);
  assert.match(report, /user/);
  assert.doesNotMatch(report, /Old system|Tool results:/);
});

test("caps contributors and sanitizes labels", () => {
  const { pi, ctx } = fixture();
  pi.getActiveTools = () => Array.from({ length: 8 }, (_, n) => `tool\n${n}`);
  pi.getAllTools = () => pi.getActiveTools().map((name) => ({ name, description: "", parameters: {} }));
  const report = contextReport(pi, ctx);
  assert.match(report, /… 3 more/);
  assert.doesNotMatch(report, /tool\n/);
});

test("theme styling preserves the plain report and usage bar stays bounded", () => {
  const { pi, ctx } = fixture();
  const theme = { fg: (_color, text) => `\x1b[36m${text}\x1b[0m`, bold: (text) => `\x1b[1m${text}\x1b[0m` };
  const styled = contextReport(pi, ctx, theme);
  assert.equal(styled.replace(/\x1b\[[0-9;]*m/g, ""), contextReport(pi, ctx));
  assert.match(styled, /\x1b\[36m/);
  assert.match(contextReport(pi, ctx), new RegExp(`━${"─".repeat(19)}`));
  ctx.getContextUsage = () => ({ tokens: 300000, contextWindow: 200000 });
  assert.match(contextReport(pi, ctx), new RegExp(`${"━".repeat(20)}  300,000`));
  ctx.getContextUsage = () => ({ tokens: 0, contextWindow: 200000 });
  assert.match(contextReport(pi, ctx), new RegExp(`${"─".repeat(20)}  0`));
});

test("command only notifies; no transcript persistence or model call", async () => {
  const { pi, ctx } = fixture();
  let command;
  register({ ...pi, registerCommand: (name, spec) => { assert.equal(name, "context"); command = spec; } });
  let notifications = 0;
  ctx.ui = { notify: (text) => { assert.match(text, /Context ·/); notifications++; } };
  ctx.hasUI = true;
  await command.handler("", ctx);
  assert.equal(notifications, 1);
  ctx.hasUI = false;
  await command.handler("", ctx);
  assert.equal(notifications, 1);
});
