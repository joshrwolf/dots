import { displayLimits } from "../policy.ts";
import { diffArrays, diffWordsWithSpace } from "diff";

// Parse the recorded unified patch, never the current contents of the file.
export interface Line {
  kind: "context" | "added" | "removed" | "note";
  text: string;
  old?: number;
  next?: number;
}
export interface Hunk {
  oldStart: number;
  newStart: number;
  lines: Line[];
}
export interface Patch {
  hunks: Hunk[];
  added: number;
  removed: number;
}

// Bound parsing and retained layout state for unusually large tool results.
export const MAX_PATCH_CHARS = displayLimits.sourceCodeUnits;
const MAX_PATCH_LINES = displayLimits.patchLines;

export function parsePatch(source: string): Patch | undefined {
  if (source.length > MAX_PATCH_CHARS) return undefined;
  const lines = source.split("\n");
  if (lines.length > MAX_PATCH_LINES) return undefined;
  const patch: Patch = { hunks: [], added: 0, removed: 0 };
  let hunk: Hunk | undefined;
  let old = 0;
  let next = 0;
  let oldLeft = 0;
  let newLeft = 0;
  let headers = 0;
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i]!;
    if (i === lines.length - 1 && raw === "") break;
    const match = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(?:.*)$/.exec(raw);
    if (match) {
      if (oldLeft !== 0 || newLeft !== 0) return undefined;
      const values = [match[1], match[2] ?? "1", match[3], match[4] ?? "1"].map(Number);
      if (!values.every(Number.isSafeInteger)) return undefined;
      [old, oldLeft, next, newLeft] = values as [number, number, number, number];
      hunk = { oldStart: old, newStart: next, lines: [] };
      patch.hunks.push(hunk);
      continue;
    }
    if (!hunk) {
      // The built-in edit produces one file's patch. Reject multi-file/malformed input.
      if ((headers === 0 && raw.startsWith("--- ")) || (headers === 1 && raw.startsWith("+++ "))) {
        headers++;
        continue;
      }
      return undefined;
    }
    if (raw === "\\ No newline at end of file") {
      if (!hunk.lines.length || hunk.lines.at(-1)?.kind === "note") return undefined;
      hunk.lines.push({ kind: "note", text: raw });
      continue;
    }
    const text = raw.slice(1);
    switch (raw[0]) {
      case " ":
        if (--oldLeft < 0 || --newLeft < 0) return undefined;
        hunk.lines.push({ kind: "context", text, old: old++, next: next++ });
        break;
      case "-":
        if (--oldLeft < 0) return undefined;
        hunk.lines.push({ kind: "removed", text, old: old++ });
        patch.removed++;
        break;
      case "+":
        if (--newLeft < 0) return undefined;
        hunk.lines.push({ kind: "added", text, next: next++ });
        patch.added++;
        break;
      default: return undefined;
    }
  }
  return patch.hunks.length && oldLeft === 0 && newLeft === 0 ? patch : undefined;
}

export interface Pair {
  left?: Line;
  right?: Line;
}

// Bounded sequence alignment pairs related replacements, leaving insertions
// opposite an empty cell instead of shifting every subsequent line.
function align(removed: Line[], added: Line[]): Pair[] {
  if (removed.length * added.length > 256 || [...removed, ...added].some((line) => line.text.length > 1000)) {
    return [...removed.map((left) => ({ left })), ...added.map((right) => ({ right }))];
  }
  const tokenize = (line: Line) => line.text.trim().match(/[\p{L}\p{N}_]+|[^\s]/gu) ?? [];
  const before = removed.map(tokenize);
  const after = added.map(tokenize);
  const scores = before.map((a) => after.map((b) => {
    if (a.length + b.length === 0) return 1;
    const changes = diffArrays(a, b, { maxEditLength: 64 });
    const common = changes?.reduce((sum, part) => sum + (!part.added && !part.removed ? part.value.length : 0), 0) ?? 0;
    const overlap = 2 * common / (a.length + b.length);
    // A shared leading keyword/identifier is a useful structural anchor even
    // when the expression changes form (concatenation → template literal).
    const anchor = overlap >= 0.2 && a[0] === b[0] && /^[\p{L}_]/u.test(a[0] ?? "") ? 0.2 : 0;
    const score = Math.min(1, overlap + anchor);
    return score >= 0.45 ? score : 0;
  }));
  const dp = Array.from({ length: removed.length + 1 }, () => new Float64Array(added.length + 1));
  for (let i = removed.length - 1; i >= 0; i--) {
    for (let j = added.length - 1; j >= 0; j--) {
      dp[i]![j] = Math.max(dp[i + 1]![j]!, dp[i]![j + 1]!, scores[i]![j]! + dp[i + 1]![j + 1]!);
    }
  }
  const rows: Pair[] = [];
  let i = 0;
  let j = 0;
  while (i < removed.length || j < added.length) {
    if (i < removed.length && j < added.length && scores[i]![j]! > 0 && dp[i]![j] === scores[i]![j]! + dp[i + 1]![j + 1]!) {
      // The branch bounds both indices; advance them only after selection.
      rows.push({ left: removed[i++]!, right: added[j++]! });
    } else if (i < removed.length && (j === added.length || dp[i + 1]![j]! >= dp[i]![j + 1]!)) {
      rows.push({ left: removed[i++]! });
    } else rows.push({ right: added[j++]! });
  }
  return rows;
}

export interface Range { start: number; end: number }

export function wordChanges(before: string, after: string): { before: Range[]; after: Range[] } {
  const ranges = { before: [] as Range[], after: [] as Range[] };
  if (before.length + after.length > 4000) return ranges;
  const parts = diffWordsWithSpace(before, after, { maxEditLength: 256, timeout: 5 });
  if (!parts) return ranges;
  let old = 0;
  let next = 0;
  for (const part of parts) {
    if (part.removed) ranges.before.push({ start: old, end: old + part.value.length });
    if (part.added) ranges.after.push({ start: next, end: next + part.value.length });
    if (!part.added) old += part.value.length;
    if (!part.removed) next += part.value.length;
  }
  return ranges;
}
export function pairLines(lines: readonly Line[]): Pair[] {
  const rows: Pair[] = [];
  let removed: Line[] = [];
  let added: Line[] = [];
  const flush = () => {
    rows.push(...align(removed, added));
    removed = [];
    added = [];
  };
  for (const line of lines) {
    if (line.kind === "removed") {
      if (added.length) flush();
      removed.push(line);
    } else if (line.kind === "added") {
      added.push(line);
    } else {
      flush();
      rows.push({ left: line, right: line });
    }
  }
  flush();
  return rows;
}
