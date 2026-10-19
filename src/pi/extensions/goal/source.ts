import { execFile } from "node:child_process";
import { constants } from "node:fs";
import { open } from "node:fs/promises";
import path from "node:path";

export interface GoalTask {
  readonly id: string;
  readonly title: string;
  readonly status: "open" | "held" | "done" | "dropped";
  readonly parent: string | null;
  readonly children: number;
  readonly owner: string | null;
  readonly lease_until: number | null;
  readonly pr: string | null;
  readonly branch: string | null;
  readonly blocked_by: readonly string[];
}
export interface Goal { readonly root: GoalTask; readonly brief: string }
export interface Progress {
  readonly total: number; readonly open: number; readonly held: number;
  readonly done: number; readonly dropped: number; readonly ready: number;
  readonly claimed: number; readonly with_pr: number;
}
export interface GoalSnapshot {
  readonly project: string;
  readonly now: number;
  readonly overview: { readonly goal: Goal; readonly tasks: readonly GoalTask[]; readonly hidden_closed: number; readonly progress: Progress };
}
export interface GoalList { readonly project: string; readonly goals: readonly Goal[] }
export interface Brief { readonly text: string; readonly warning?: string }
export interface GoalSource {
  list(cwd: string, signal: AbortSignal): Promise<GoalList>;
  show(cwd: string, id: string, signal: AbortSignal, history?: boolean): Promise<GoalSnapshot>;
  brief(file: string): Promise<Brief>;
}
export const BRIEF_BYTES = 24 * 1024;

// Decode the fields this consumer uses; ignore additional CLI fields. Invalid
// data is an unavailable observation, never an empty/successful goal.
function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid dots goal response");
  return value as Record<string, unknown>;
}
function string(value: unknown): string {
  if (typeof value !== "string") throw new Error("Invalid dots goal text");
  return value;
}
function count(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw new Error("Invalid dots goal count");
  return value;
}
function nullableString(value: unknown): string | null { return value === null ? null : string(value); }
function array(value: unknown): unknown[] {
  if (!Array.isArray(value)) throw new Error("Invalid dots goal list");
  return value;
}
function task(value: unknown): GoalTask {
  const v = record(value);
  if (!["open", "held", "done", "dropped"].includes(string(v.status))) throw new Error("Invalid dots task status");
  return { id: string(v.id), title: string(v.title), status: v.status as GoalTask["status"],
    parent: nullableString(v.parent), children: count(v.children), owner: nullableString(v.owner),
    lease_until: v.lease_until === null ? null : count(v.lease_until), pr: nullableString(v.pr), branch: nullableString(v.branch),
    blocked_by: array(v.blocked_by).map(string) };
}
function goal(value: unknown): Goal {
  const v = record(value), root = task(v.root), brief = string(v.brief);
  if (root.parent !== null || !path.isAbsolute(brief)) throw new Error("Invalid dots goal root or brief reference");
  return { root, brief };
}
export function decodeList(value: unknown): GoalList {
  const v = record(value);
  return { project: string(v.project), goals: array(v.goals).map(goal) };
}
export function decodeSnapshot(value: unknown): GoalSnapshot {
  const v = record(value), overview = record(v.overview), p = record(overview.progress);
  return { project: string(v.project), now: count(v.now), overview: {
    goal: goal(overview.goal), tasks: array(overview.tasks).map(task), hidden_closed: count(overview.hidden_closed),
    progress: { total: count(p.total), open: count(p.open), held: count(p.held), done: count(p.done),
      dropped: count(p.dropped), ready: count(p.ready), claimed: count(p.claimed), with_pr: count(p.with_pr) },
  } };
}

async function query(cwd: string, args: string[], signal: AbortSignal): Promise<unknown> {
  return new Promise((resolve, reject) => {
    execFile("dots", ["--directory", cwd, "--json", "task", "goal", ...args],
      { cwd, signal, timeout: 10_000, maxBuffer: 1024 * 1024, encoding: "utf8", killSignal: "SIGKILL" },
      (error, stdout, stderr) => {
        if (error) {
          reject(new Error(`dots goal query failed: ${stderr.trim().slice(0, 512) || error.message}. Ensure the installed dots supports 'task goal'.`));
          return;
        }
        try { resolve(JSON.parse(stdout)); } catch { reject(new Error("dots returned invalid goal JSON")); }
      });
  });
}

/** Bounded reads even for a mistaken FIFO/device reference; no recursive links or memory assumptions. */
export async function readBrief(file: string): Promise<Brief> {
  try {
    const handle = await open(file, constants.O_RDONLY | constants.O_NONBLOCK);
    try {
      const stat = await handle.stat();
      if (!stat.isFile()) throw new Error("not a regular file");
      const buffer = Buffer.alloc(BRIEF_BYTES + 1);
      let bytes = 0;
      while (bytes < buffer.length) {
        const read = await handle.read(buffer, bytes, buffer.length - bytes, null);
        if (read.bytesRead === 0) break;
        bytes += read.bytesRead;
      }
      const truncated = bytes > BRIEF_BYTES;
      // Streaming decoder omits a partial final UTF-8 sequence at the byte bound.
      const text = new TextDecoder().decode(buffer.subarray(0, Math.min(bytes, BRIEF_BYTES)), { stream: truncated });
      return { text, ...(truncated ? { warning: `Brief truncated at ${BRIEF_BYTES} bytes; read ${file} for the rest.` } : {}) };
    } finally { await handle.close(); }
  } catch (error) {
    return { text: "", warning: `Brief unavailable: ${String(error).slice(0, 512)}` };
  }
}

export const dotsSource: GoalSource = {
  async list(cwd, signal) { return decodeList(await query(cwd, ["list"], signal)); },
  async show(cwd, id, signal, history = false) { return decodeSnapshot(await query(cwd, ["show", id, ...(history ? ["--all"] : [])], signal)); },
  brief: readBrief,
};
