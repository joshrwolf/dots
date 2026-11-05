import {createBashToolDefinition} from '@earendil-works/pi-coding-agent';

// Runs a native bash command the way a model call does and records what it
// sees. With a task ID and project it claims the task there without --owner,
// so dots records the actor the shell carries.
export default function agentObserver(pi) {
  pi.registerCommand('fixture-agent', {
    description: 'Observe the actor a native shell carries',
    async handler(args, ctx) {
      const [step, task, project] = args.trim().split(/\s+/);
      const bash = createBashToolDefinition(ctx.cwd);
      const claim = task
        ? `dots --json task claim ${task} --project '${project}' >/dev/null && `
        : '';
      const result = await bash.execute(
        'fixture',
        {
          command: `${claim}printf '%s\\n%s' "$PI_SESSION_ID" "$DOTS_AGENT_OWNER"`,
        },
        undefined,
        undefined,
        ctx,
      );
      const text = result.content
        .map(part => (part.type === 'text' ? part.text : ''))
        .join('');
      if (result.isError) throw new Error(text);
      const [session = '', owner = ''] = text.split('\n');
      globalThis.fixtureAgentReport = {step, shell: {session, owner}};
    },
  });
}
