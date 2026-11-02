import path from 'node:path';
import type {ExtensionAPI} from '@earendil-works/pi-coding-agent';

/**
 * Rex's agent setup gives Claude Code and Codex a session-start hook that adds
 * Rex's own orientation to the agent's context. Pi has no command hooks, so
 * this asks the same command at session start. Rex owns the text, which
 * changes with each Rex release. Program status belongs to the status
 * extension.
 */

/** Runs a program and resolves with its stdout; the test seam for the Rex invocation. */
export type Run = (file: string, args: readonly string[]) => Promise<string>;

/** The host's process runner owns the deadline, output cap and kill escalation. */
const hostRun =
  (pi: ExtensionAPI): Run =>
  async (file, args) => {
    const result = await pi.exec(file, [...args], {
      timeout: 5000,
      maxBuffer: 256 * 1024,
    });
    if (result.killed || result.code !== 0)
      throw new Error(result.stderr.trim() || `${file} failed`);
    return result.stdout;
  };

/** Rex's orientation for this terminal, or undefined outside Rex or on any failure. */
export async function rexContext(
  env: NodeJS.ProcessEnv,
  run: Run,
): Promise<string | undefined> {
  // A worker is not the session's owner; the orientation (naming the session,
  // and so on) is for the agent the user talks to.
  if (!env.REX_SESSION || env.DOTS_WORKER_ID) return undefined;
  try {
    const rex = env.REX_BIN_DIR ? path.join(env.REX_BIN_DIR, 'rex') : 'rex';
    const text = (
      await run(rex, ['__internal', 'session-start', '--agent', 'pi'])
    ).trim();
    return text || undefined;
  } catch {
    // Matches Rex's own hook, which never blocks a session it cannot describe.
    return undefined;
  }
}

export default function (
  pi: ExtensionAPI,
  env: NodeJS.ProcessEnv = process.env,
  run: Run = hostRun(pi),
) {
  let context: string | undefined;
  pi.on('session_start', async () => {
    context = await rexContext(env, run);
  });
  pi.on('session_shutdown', () => {
    context = undefined;
  });
  pi.on('before_agent_start', event => {
    const sections = event.systemPromptOptions.sections;
    if (context) sections.dots_rex = context;
    else delete sections.dots_rex;
  });
}
