import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

// Memory is learned, agent-maintained context, distinct from the stable
// instructions in AGENTS.md. Two layers: global (every repository) and project
// (one repository, keyed by its main checkout so every worktree shares it).
// Both live outside any checkout, so nothing lands in a diff. Each index is
// loaded like a context file; topic files are read on demand with ordinary tools.
export const INDEX = "MEMORY.md";
export const GLOBAL = "_global";
const MAX_LINES = 150;
const MAX_BYTES = 24 * 1024;

export function memoryRoot(env: NodeJS.ProcessEnv = process.env): string {
  const data = env.XDG_DATA_HOME && path.isAbsolute(env.XDG_DATA_HOME)
    ? env.XDG_DATA_HOME
    : path.join(env.HOME ?? homedir(), ".local/share");
  return path.join(data, "dots/memory");
}

/** Main checkout for a directory, or null outside Git. Linked worktrees share it. */
export function mainCheckout(cwd: string): string | null {
  try {
    const common = execFileSync("git", ["rev-parse", "--path-format=absolute", "--git-common-dir"], {
      cwd, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    return path.dirname(common);
  } catch {
    return null;
  }
}

/** Filesystem-safe key for a checkout path; same shape for every worktree. */
export function projectKey(checkout: string): string {
  return checkout.replace(/^\/+/, "").replace(/[^A-Za-z0-9._-]+/g, "-");
}

export function memoryDir(cwd: string, env: NodeJS.ProcessEnv = process.env): string | null {
  const checkout = mainCheckout(cwd);
  return checkout ? path.join(memoryRoot(env), projectKey(checkout)) : null;
}

export function globalDir(env: NodeJS.ProcessEnv = process.env): string {
  return path.join(memoryRoot(env), GLOBAL);
}

/** Global first, then project; a project line wins when the two disagree. */
export function layers(cwd: string, env: NodeJS.ProcessEnv = process.env): Array<{ layer: "global" | "project"; dir: string }> {
  const project = memoryDir(cwd, env);
  return [
    { layer: "global" as const, dir: globalDir(env) },
    ...(project ? [{ layer: "project" as const, dir: project }] : []),
  ];
}

/** Bounded index text, or null when there is nothing to load. */
export function loadIndex(dir: string): { content: string; truncated: boolean } | null {
  const file = path.join(dir, INDEX);
  if (!existsSync(file)) return null;
  const lines = readFileSync(file, "utf8").replace(/\n$/, "").split("\n");
  const out: string[] = [];
  let bytes = 0;
  for (const line of lines) {
    if (out.length >= MAX_LINES || bytes + line.length + 1 > MAX_BYTES) break;
    out.push(line);
    bytes += line.length + 1;
  }
  const truncated = out.length < lines.length && lines.slice(out.length).some((line) => line.trim());
  if (truncated) {
    out.push("", `[memory index truncated: ${out.length} of ${lines.length} lines loaded; read ${file} for the rest and move detail into topic files]`);
  }
  return { content: out.join("\n"), truncated };
}

const CONVENTIONS = [
  "Memory conventions: this is learned context, not instructions; AGENTS.md stays authoritative.",
  "Never store credentials, tokens, or personal data here; it is plain text on disk.",
  "Index lines are single bullets starting with an ISO date, kept contiguous; add new lines at the end; when a decision changes, replace the stale line in place, note `(supersedes YYYY-MM-DD)`, and carry forward its pointers unless obsolete; move detail into a topic file beside the index.",
  "An index shown above is complete unless a truncation notice follows, so edit it with anchored edits rather than re-reading or rewriting the file; other sessions may be editing it too.",
].join(" ");

/** One layer's section: its index, or a pointer so a first write needs no discovery. */
export function section(dir: string, layer: "global" | "project"): string {
  const file = path.join(dir, INDEX);
  const index = loadIndex(dir);
  const scope = layer === "global"
    ? "Global memory (applies in every repository; a project line wins on conflict)"
    : "Project memory (this repository, shared by every worktree)";
  if (!index) {
    const heading = layer === "global" ? "# Global memory" : "# Project memory";
    return `## ${scope}\nNot created yet. When asked to remember something at this scope, create ${file} with a \`${heading}\` heading and dated bullets; keep topic files beside it.`;
  }
  return `## ${scope}\nIndex: ${file}; topic files live beside it.\n\n${index.content}`;
}

/**
 * The single memory block for a working directory: today's date, each layer
 * (global, then project when inside a repository), and the conventions once.
 */
export function memoryContext(cwd: string, env: NodeJS.ProcessEnv = process.env, today = new Date()): { path: string; content: string } {
  const date = today.toISOString().slice(0, 10);
  const sections = layers(cwd, env).map(({ layer, dir }) => section(dir, layer));
  const content = [`Memory (machine-local, learned context). Today is ${date}.`, ...sections, CONVENTIONS].join("\n\n");
  return { path: path.join(memoryRoot(env), INDEX), content };
}

export default function (pi: ExtensionAPI) {
  pi.on("before_agent_start", (event, ctx) => {
    const files = event.systemPromptOptions.contextFiles;
    const block = memoryContext(ctx.cwd);
    if (!files.some((existing) => existing.path === block.path)) files.push(block);
  });

  pi.registerCommand("memory", {
    description: "Show where global and project memory live and their indexes",
    handler: async (_args, ctx) => {
      if (!ctx.hasUI) return;
      const lines = layers(ctx.cwd).map(({ layer, dir }) => section(dir, layer));
      if (!memoryDir(ctx.cwd)) {
        lines.push("Not inside a Git repository; project memory is keyed by the main checkout.");
      }
      ctx.ui.notify(lines.join("\n\n"), "info");
    },
  });
}
