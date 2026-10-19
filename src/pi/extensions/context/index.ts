import {
  estimateTokens,
  formatSkillsForPrompt,
  type ExtensionAPI,
  type ExtensionCommandContext,
  type Theme,
} from "@earendil-works/pi-coding-agent";

type Row = { name: string; tokens: number };
const tokens = (text: string) => Math.ceil(text.length / 4);
const number = (value: number) => value.toLocaleString("en-US");
// Labels only: never print prompt bodies, message content, or terminal controls.
const label = (text: string) => text.replace(/[\x00-\x1f\x7f-\x9f]/g, " ");

export function contextReport(pi: Pick<ExtensionAPI, "getAllTools" | "getActiveTools">,
  ctx: Pick<ExtensionCommandContext,
    "getContextUsage" | "getSystemPrompt" | "getSystemPromptOptions" | "sessionManager" | "isIdle" | "model">,
  theme?: Pick<Theme, "fg" | "bold">,
): string {
  const usage = ctx.getContextUsage();
  const contextWindow = usage?.contextWindow ?? ctx.model?.contextWindow;
  const percent = (count: number) => contextWindow && contextWindow > 0
    ? `${(count / contextWindow * 100).toFixed(2)}%`
    : "n/a";
  const accent = (text: string) => theme?.fg("accent", text) ?? text;
  const muted = (text: string) => theme?.fg("dim", text) ?? text;
  const heading = (text: string) => accent(theme?.bold(text) ?? text);
  const known = usage?.tokens != null && contextWindow != null && contextWindow > 0;
  const filled = known ? Math.max(0, Math.min(20, Math.round(usage.tokens! / contextWindow * 20))) : 0;
  const lines = [
    heading("Context") + muted(ctx.isIdle() ? " · snapshot" : " · Agent is active"),
    known
      ? `  ${accent("━".repeat(filled))}${muted("─".repeat(20 - filled))}  ${number(usage.tokens!)} / ${number(contextWindow)} tokens (${percent(usage.tokens!)})`
      : muted("  Pi context usage: unknown"),
    "",
    muted("    Tokens   Window   Source"),
  ];
  const row = (name: string, count: number, depth = 0) => {
    const prefix = depth ? muted(`${"  ".repeat(depth - 1)}└─ `) : "";
    return `  ~${number(count).padStart(7)}  ${muted(percent(count).padStart(7))}  ${prefix}${depth ? label(name) : heading(label(name))}`;
  };
  const contributors = (items: Row[], depth = 1) => {
    const sorted = [...items].sort((a, b) => b.tokens - a.tokens);
    for (const item of sorted.slice(0, 5)) lines.push(row(item.name, item.tokens, depth));
    if (sorted.length > 5) lines.push(muted(`                      ${"  ".repeat(depth)}… ${sorted.length - 5} more`));
  };

  const prompt = ctx.getSystemPrompt();
  const options = ctx.getSystemPromptOptions();
  lines.push(row("System prompt", tokens(prompt)));
  // Only attribute text that is actually present. Forced/rewritten prompts may
  // omit configured resources. Consume matches once to avoid overlapping claims.
  let remaining = prompt;
  const take = (text: string) => {
    if (!text || !remaining.includes(text)) return 0;
    remaining = remaining.replace(text, "");
    return tokens(text);
  };
  const skills: Row[] = [];
  for (const skill of options.skills ?? []) {
    const entry = formatSkillsForPrompt([skill]).match(/  <skill>[\s\S]*?  <\/skill>/)?.[0];
    const count = take(entry ?? "");
    if (count) skills.push({ name: skill.name, tokens: count });
  }
  const files: Row[] = [];
  for (const file of options.contextFiles ?? []) {
    const count = take(file.content);
    if (count) files.push({ name: file.path, tokens: count });
  }
  for (const [name, items] of [["Instruction files", files], ["Skill catalog entries", skills]] as const) {
    lines.push(row(`${name} (${items.length})`, items.reduce((sum, item) => sum + item.tokens, 0), 1));
    contributors(items, 2);
  }
  lines.push(row("Other text / wrappers", tokens(remaining), 1));

  const active = new Set(pi.getActiveTools());
  // getAllTools exposes registered schemas, not prepareLoadout's rewritten
  // descriptions. Codemode may also hide declarations while keeping tools callable.
  // Don't present raw schema sizes as request costs in either codemode mode.
  if (active.has("codemode")) {
    lines.push("", heading("Tool definitions"),
      muted("  Estimate unavailable: codemode rewrites tool declarations."));
  } else {
    const tools = pi.getAllTools().filter((tool) => active.has(tool.name)).map((tool) => ({
      name: tool.name,
      tokens: tokens(JSON.stringify({ name: tool.name, description: tool.description, parameters: tool.parameters })),
    }));
    lines.push("", row(`Registered tool schemas (${tools.length})`, tools.reduce((sum, tool) => sum + tool.tokens, 0)));
    contributors(tools);
  }

  // Projection respects the active branch, compaction, and context edits.
  // Current system/tool inventory above is separate from historical system deltas.
  const conversation = new Map<string, number>();
  for (const message of ctx.sessionManager.buildSessionProjection().messages) {
    if (message.role === "system") continue;
    const name = message.role === "toolResult" ? `Tool results: ${message.toolName}` : message.role;
    conversation.set(name, (conversation.get(name) ?? 0) + estimateTokens(message));
  }
  lines.push("", row("Conversation", [...conversation.values()].reduce((a, b) => a + b, 0)));
  contributors([...conversation].map(([name, count]) => ({ name, tokens: count })));
  lines.push("", muted("~ estimated tokens · % of full model window · nested rows are subsets"),
    muted("Skill bodies / invoked templates → conversation; unused templates → no cost."),
    muted("Estimates aren't reconciled to Pi's total; request transforms / history may differ."));
  return lines.join("\n");
}

export default function (pi: ExtensionAPI) {
  pi.registerCommand("context", {
    description: "Show estimated context sources: instructions, skills, tools, conversation",
    handler: async (_args, ctx) => {
      if (!ctx.hasUI) return;
      ctx.ui.notify(contextReport(pi, ctx, ctx.mode === "tui" ? ctx.ui.theme : undefined), "info");
    },
  });
}
