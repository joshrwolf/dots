import {
  getCurrentSystemMessage,
  getSystemMessageText,
} from '@earendil-works/pi-ai';
import {
  SettingsManager,
  estimateTokens,
  formatSkillsForPrompt,
  type ExtensionAPI,
  type ExtensionCommandContext,
  type Theme,
} from '@earendil-works/pi-coding-agent';

type Row = {name: string; tokens: number};
const tokens = (text: string) => Math.ceil(text.length / 4);
const number = (value: number) => value.toLocaleString('en-US');
// Labels only: never print prompt bodies, message content, or terminal controls.
const label = (text: string) => text.replace(/[\x00-\x1f\x7f-\x9f]/g, ' ');

export function contextReport(
  pi: Pick<ExtensionAPI, 'getAllTools' | 'getActiveTools' | 'getSettings'>,
  ctx: Pick<
    ExtensionCommandContext,
    | 'getContextUsage'
    | 'getSystemPrompt'
    | 'getSystemPromptOptions'
    | 'sessionManager'
    | 'isIdle'
    | 'model'
  >,
  theme?: Pick<Theme, 'fg' | 'bold'>,
): string {
  const usage = ctx.getContextUsage();
  const contextWindow = usage?.contextWindow ?? ctx.model?.contextWindow;
  const percent = (count: number) =>
    contextWindow && contextWindow > 0
      ? `${((count / contextWindow) * 100).toFixed(2)}%`
      : 'n/a';
  const accent = (text: string) => theme?.fg('accent', text) ?? text;
  const muted = (text: string) => theme?.fg('dim', text) ?? text;
  const heading = (text: string) => accent(theme?.bold(text) ?? text);
  const known =
    usage?.tokens != null && contextWindow != null && contextWindow > 0;
  const filled = known
    ? Math.max(
        0,
        Math.min(20, Math.round((usage.tokens! / contextWindow) * 20)),
      )
    : 0;
  const settings = ctx.model
    ? SettingsManager.inMemory(pi.getSettings()).getCompactionSettings(
        ctx.model,
      )
    : undefined;
  const trigger =
    settings?.enabled && contextWindow !== undefined
      ? Math.max(0, contextWindow - settings.reserveTokens)
      : undefined;
  const compaction =
    settings?.enabled === false
      ? 'Automatic compaction: disabled'
      : trigger === undefined
        ? 'Automatic compaction: unknown'
        : `Automatic compaction: above ${number(trigger)} tokens${usage?.tokens == null ? '; remaining unknown' : usage.tokens > trigger ? `; ~${number(usage.tokens - trigger)} over trigger` : `; ~${number(trigger - usage.tokens)} remaining`}`;
  const lines = [
    heading('Context') +
      muted(ctx.isIdle() ? ' · snapshot' : ' · Agent is active'),
    known
      ? `  ${accent('━'.repeat(filled))}${muted('─'.repeat(20 - filled))}  ${number(usage.tokens!)} / ${number(contextWindow)} tokens (${percent(usage.tokens!)})`
      : muted('  Pi context usage: unknown'),
    muted(`  ${compaction}`),
    '',
    muted('    Tokens   Window   Source'),
  ];
  const row = (name: string, count: number, depth = 0) => {
    const prefix = depth ? muted(`${'  '.repeat(depth - 1)}└─ `) : '';
    return `  ~${number(count).padStart(7)}  ${muted(percent(count).padStart(7))}  ${prefix}${depth ? label(name) : heading(label(name))}`;
  };
  const contributors = (items: Row[], depth = 1) => {
    const sorted = [...items].sort((a, b) => b.tokens - a.tokens);
    for (const item of sorted.slice(0, 5))
      lines.push(row(item.name, item.tokens, depth));
    if (sorted.length > 5)
      lines.push(
        muted(
          `                      ${'  '.repeat(depth)}… ${sorted.length - 5} more`,
        ),
      );
  };

  const messages = ctx.sessionManager.buildSessionProjection().messages;
  const current = getCurrentSystemMessage(messages);
  const prompt = current
    ? getSystemMessageText(current)
    : ctx.getSystemPrompt();
  // The host exposes base options here, not the extension-mutated run options.
  // Use them only as resource labels; the transcript owns effective prompt state.
  const options = ctx.getSystemPromptOptions();
  lines.push(
    row(
      current
        ? 'System prompt (recorded effective)'
        : 'System prompt (host fallback; no recorded request)',
      tokens(prompt),
    ),
  );
  // Only attribute text that is actually present. Forced/rewritten prompts may
  // omit configured resources. Consume matches once to avoid overlapping claims.
  let remaining = prompt;
  const take = (text: string) => {
    if (!text || !remaining.includes(text)) return 0;
    remaining = remaining.replace(text, '');
    return tokens(text);
  };
  const skills: Row[] = [];
  for (const skill of options.skills ?? []) {
    const entry = formatSkillsForPrompt([skill]).match(
      /  <skill>[\s\S]*?  <\/skill>/,
    )?.[0];
    const count = take(entry ?? '');
    if (count) skills.push({name: skill.name, tokens: count});
  }
  const files: Row[] = [];
  for (const file of options.contextFiles ?? []) {
    const count = take(file.content);
    if (count) files.push({name: file.path, tokens: count});
  }
  const sections: Row[] = [];
  for (const [name, content] of Object.entries(current?.sections ?? {})) {
    if (!name.startsWith('dots_') || content === null) continue;
    const count = take(content);
    if (count) sections.push({name, tokens: count});
  }
  for (const [name, items] of [
    ['Instruction files', files],
    ['Skill catalog entries', skills],
    ['Companion sections', sections],
  ] as const) {
    lines.push(
      row(
        `${name} (${items.length})`,
        items.reduce((sum, item) => sum + item.tokens, 0),
        1,
      ),
    );
    contributors(items, 2);
  }
  lines.push(row('Other text / wrappers', tokens(remaining), 1));

  const active = new Set(pi.getActiveTools());
  // getAllTools exposes registered schemas, not prepareLoadout's rewritten
  // descriptions. Codemode may also hide declarations while keeping tools callable.
  // Don't present raw schema sizes as request costs in either codemode mode.
  if (active.has('codemode')) {
    lines.push(
      '',
      heading('Tool definitions'),
      muted('  Estimate unavailable: codemode rewrites tool declarations.'),
    );
  } else {
    const tools = pi
      .getAllTools()
      .filter(tool => active.has(tool.name))
      .map(tool => ({
        name: tool.name,
        tokens: tokens(
          JSON.stringify({
            name: tool.name,
            description: tool.description,
            parameters: tool.parameters,
          }),
        ),
      }));
    lines.push(
      '',
      row(
        `Registered tool schemas (${tools.length})`,
        tools.reduce((sum, tool) => sum + tool.tokens, 0),
      ),
    );
    contributors(tools);
  }

  // Canonical projection respects branches, compaction and context edits. This
  // historical view overlaps the current inventory above; never add the two.
  const conversation = new Map<string, number>();
  const updates = new Map<string, number>();
  let seenSystem = false;
  let updateCount = 0;
  let updateTokens = 0;
  for (const message of messages) {
    const count = estimateTokens(message);
    let name: string;
    if (message.role === 'system') {
      name = seenSystem ? 'System updates' : 'System checkpoint';
      if (seenSystem) {
        updateCount++;
        updateTokens += count;
        for (const [section, content] of Object.entries(message.sections ?? {}))
          updates.set(
            section,
            (updates.get(section) ?? 0) + tokens(content ?? ''),
          );
      }
      seenSystem = true;
    } else if (message.role === 'toolResult')
      name = `Tool results: ${message.toolName}`;
    else if (message.role === 'custom') name = `Events: ${message.customType}`;
    else name = message.role;
    conversation.set(name, (conversation.get(name) ?? 0) + count);
  }
  lines.push(
    '',
    row(
      'Projected conversation (includes system history)',
      [...conversation.values()].reduce((a, b) => a + b, 0),
    ),
  );
  contributors(
    [...conversation].map(([name, count]) => ({name, tokens: count})),
  );
  lines.push(
    '',
    row(
      `System updates (${updateCount}; subset of projected conversation)`,
      updateTokens,
    ),
  );
  contributors([...updates].map(([name, count]) => ({name, tokens: count})));
  lines.push(
    '',
    muted(
      '~ estimated tokens · % of full model window · nested rows are subsets',
    ),
    muted(
      'Skill bodies / invoked templates → conversation; unused templates → no cost.',
    ),
    muted(
      'Current inventory and projected conversation overlap; do not add them. Estimates are not billing or cache-hit measurements. Recorded prompt state excludes forced/request-local overrides; provider folding may differ.',
    ),
  );
  return lines.join('\n');
}

export default function (pi: ExtensionAPI) {
  pi.registerCommand('context', {
    description:
      'Show estimated context sources: instructions, skills, tools, conversation',
    handler: async (_args, ctx) => {
      if (!ctx.hasUI) return;
      ctx.ui.notify(
        contextReport(pi, ctx, ctx.mode === 'tui' ? ctx.ui.theme : undefined),
        'info',
      );
    },
  });
}
