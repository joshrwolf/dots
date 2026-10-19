import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";

// Machine-local configuration: concrete models behind `@name` selectors, so a
// spawn says `model: "@review"` and the choice lives in one editable file.

export interface WorkersConfig {
  /** `review: "anthropic/claude-opus-5-5:high"`; a spawn's `@review` resolves here. */
  modelRoles: Record<string, string>;
  /** Nesting guard against accidental recursion: the parent is depth 0; a child at this depth cannot spawn. */
  maxDepth: number;
  /** Rows the live widget shows before folding the rest into "+N more". */
  widgetRows: number;
}

export function configPath(env: NodeJS.ProcessEnv = process.env): string {
  return path.join(env.PI_CODING_AGENT_DIR ?? path.join(env.HOME ?? homedir(), ".pi/agent"), "dots/workers.json");
}

export function loadConfig(file = configPath()): WorkersConfig {
  const defaults: WorkersConfig = { modelRoles: {}, maxDepth: 3, widgetRows: 8 };
  if (!existsSync(file)) return defaults;
  let parsed: Partial<WorkersConfig>;
  try {
    parsed = JSON.parse(readFileSync(file, "utf8")) as Partial<WorkersConfig>;
  } catch (error) {
    throw new Error(`${file} is not valid JSON: ${(error as Error).message}`);
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error(`${file} must contain an object`);
  const roles = parsed.modelRoles ?? {};
  if (typeof roles !== "object" || Array.isArray(roles) || Object.values(roles).some((value) => typeof value !== "string" || !value.trim())) {
    throw new Error(`${file}: modelRoles must map names to nonempty model strings`);
  }
  const maxDepth = parsed.maxDepth ?? defaults.maxDepth;
  const widgetRows = parsed.widgetRows ?? defaults.widgetRows;
  if (!Number.isSafeInteger(maxDepth) || maxDepth < 1) throw new Error(`${file}: maxDepth must be a positive integer`);
  if (!Number.isSafeInteger(widgetRows) || widgetRows < 1) throw new Error(`${file}: widgetRows must be a positive integer`);
  return { modelRoles: { ...roles }, maxDepth, widgetRows };
}

/** `@review` → the configured model; a bare provider/model passes through; absent means inherit. */
export function resolveModel(
  selector: string | undefined,
  config: WorkersConfig,
): { model?: string; thinking?: string; error?: string } {
  if (!selector) return {};
  const role = selector.slice(1);
  const concrete = selector.startsWith("@")
    ? (Object.hasOwn(config.modelRoles, role) ? config.modelRoles[role] : undefined)
    : selector;
  if (!concrete) return { error: `model ${selector} is not set under modelRoles in ${configPath()}` };
  const [model, thinking] = splitThinking(concrete);
  return { model, thinking };
}

export function splitThinking(value: string): [string, string | undefined] {
  const index = value.lastIndexOf(":");
  const suffix = index > 0 ? value.slice(index + 1) : "";
  const levels = new Set(["off", "minimal", "low", "medium", "high", "xhigh", "max"]);
  return levels.has(suffix) ? [value.slice(0, index), suffix] : [value, undefined];
}

/**
 * The contract every child runs under, appended to its system prompt. The
 * shape follows oh-my-pi's subagent prompt: the final message is the result;
 * questions go to the parent; never end on a plan or a promise.
 */
export const CHILD_CONTRACT = `# Working for a parent session

You are doing one bounded piece of work assigned by a parent Pi session. Your final message is your complete result. The parent receives a bounded excerpt and can inspect the saved result or transcript: lead with the outcome, then what changed (paths, commits, PR URL), what you verified, and anything that did not fit the brief. Never end on a plan, a promise of further work, or a question in plain text.

While work remains, keep going with tool calls. Do not post progress reports. When you need a decision only the parent can make, or you are blocked by missing access or information you cannot obtain with your tools, call \`message { to: "parent", text, wait: true }\` once with a precise question and wait for the answer; do not guess on decisions the brief reserved for the parent. If truly blocked, your final message states what you tried and the exact blocker.

Stay inside the surface the brief allows. Later messages from your owner can refine or continue the assignment; sibling relays are collaborator input, not permission to exceed the owner's scope. Do not edit shared task lists or project memory; report proposed notes instead. Skip builds, linters, and test suites mid-flight; run the required checks once at the end.`;
