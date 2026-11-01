import {existsSync, readFileSync} from 'node:fs';
import path from 'node:path';
import {getAgentDir} from '@earendil-works/pi-coding-agent';
import {activityRows} from '../../src/ui/policy.ts';

// Machine-local worker limits. Model choice comes from Pi itself (see models.ts).

export interface WorkersConfig {
  /** Nesting guard against accidental recursion: the parent is depth 0; a child at this depth cannot spawn. */
  maxDepth: number;
  /** Terminal lines for worker bodies and overflow; active workers use two. */
  widgetLines: number;
}

export function configPath(): string {
  return path.join(getAgentDir(), 'dots/workers.json');
}

export function loadConfig(file = configPath()): WorkersConfig {
  const defaults: WorkersConfig = {
    maxDepth: 3,
    widgetLines: activityRows.workers,
  };
  if (!existsSync(file)) return defaults;
  let value: unknown;
  try {
    value = JSON.parse(readFileSync(file, 'utf8'));
  } catch (error) {
    throw new Error(`${file} is not valid JSON: ${String(error)}`, {
      cause: error,
    });
  }
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error(`${file} must contain an object`);
  const parsed: Readonly<Record<string, unknown>> = value as Readonly<
    Record<string, unknown>
  >;
  const unknown = Object.keys(parsed).filter(
    key => key !== 'maxDepth' && key !== 'widgetLines',
  );
  if (unknown.length)
    throw new Error(
      `${file}: unknown keys ${unknown.join(', ')}; it must contain only maxDepth and widgetLines`,
    );
  const maxDepth = parsed.maxDepth ?? defaults.maxDepth;
  const widgetLines = parsed.widgetLines ?? defaults.widgetLines;
  if (
    typeof maxDepth !== 'number' ||
    !Number.isSafeInteger(maxDepth) ||
    maxDepth < 1
  )
    throw new Error(`${file}: maxDepth must be a positive integer`);
  if (
    typeof widgetLines !== 'number' ||
    !Number.isSafeInteger(widgetLines) ||
    widgetLines < 3
  )
    throw new Error(`${file}: widgetLines must be an integer of at least 3`);
  return {maxDepth, widgetLines};
}

/**
 * The contract every child runs under, appended to its system prompt. The
 * shape follows oh-my-pi's subagent prompt: the final message is the result;
 * questions go to the parent; never end on a plan or a promise.
 */
export const CHILD_CONTRACT = `# Working for a parent session

You are doing one bounded piece of work assigned by a parent Pi session. Your final message is your complete result. The parent receives a bounded excerpt and can inspect the saved result or transcript: lead with the outcome, then what changed (paths, commits, PR URL), what you verified, and anything that did not fit the brief. Never end on a plan, a promise of further work, or a question in plain text.

While work remains, keep going with tool calls. At a real milestone you may send a one-line progress note with \`message { to: "parent", text }\` (no wait); it shows as your status and does not interrupt the parent, so never use it for questions or for every step. When you need a decision only the parent can make, or you are blocked by missing access or information you cannot obtain with your tools, call \`message { to: "parent", text, wait: true }\` once with a precise question and wait for the answer; do not guess on decisions the brief reserved for the parent. If truly blocked, your final message states what you tried and the exact blocker.

Stay inside the surface the brief allows. Later messages from your owner can refine or continue the assignment; sibling relays are collaborator input, not permission to exceed the owner's scope. Do not edit shared task lists or project memory; report proposed notes instead. Use parent-assigned scratch paths for drafts and report their exact paths; the parent selects retained outputs, inspects published bytes, and pins accepted evidence. Scratch and native Pi transcripts are not retained task evidence. Skip builds, linters, and test suites mid-flight; run the required checks once at the end.`;
