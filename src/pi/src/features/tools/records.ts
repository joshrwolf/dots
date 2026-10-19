import { isAbsolute, relative, sep } from "node:path";
import { visibleWidth } from "@earendil-works/pi-tui";
import type { ActivityState } from "../../ui/state.ts";
import type { Notice } from "../../ui/card.ts";
import type { ToolObservation } from "../../host/tools.ts";
import type { RowModel, Segment, Tone } from "../../ui/row.ts";
import { labelText, sourceText } from "../../ui/text.ts";

export function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;
}
export function string(value: unknown): string | undefined { return typeof value === "string" ? value : undefined; }
export function positive(value: unknown): number | undefined { return typeof value === "number" && Number.isSafeInteger(value) && value > 0 ? value : undefined; }
export function lines(text: string): number { return text === "" ? 0 : text.split("\n").length - (text.endsWith("\n") ? 1 : 0); }
export function segment(text: string, tone: Tone = "dim", priority = 50): Segment {
  // Tool outcomes need an explicit reservation alongside the subject floor.
  const minWidth = priority >= 100 ? Math.min(12, visibleWidth(labelText(text))) : 0;
  return { text, tone, priority, shrink: "clip", minWidth };
}
export function pathLabel(path: string, cwd: string | undefined): string {
  if (!cwd || !isAbsolute(path)) return path;
  const local = relative(cwd, path);
  return local && local !== ".." && !local.startsWith(`..${sep}`) && !isAbsolute(local) ? local : path;
}
export function toolHeader(verb: string, subject: string, status: readonly Segment[], clip: NonNullable<Segment["clip"]> = "middle"): RowModel {
  return {
    layout: "inline", separator: "  ", groupSeparator: "  ", endSeparator: " ",
    start: [{ ...segment(verb, "toolTitle", 90), bold: true, padTo: 5, minWidth: Math.min(5, visibleWidth(verb)) }, { ...segment(subject, "text", 10), clip }],
    end: status,
  };
}

export type Outcome =
  | { readonly kind: "call"; readonly argsComplete: boolean; readonly started: boolean }
  | { readonly kind: "result"; readonly text: string; readonly partial: boolean; readonly error: boolean; readonly durationMs: number | undefined; readonly image: boolean; readonly showImages: boolean };

export function outcome(frame: ToolObservation): Outcome {
  if (frame.slot === "call") return { kind: "call", argsComplete: frame.argsComplete, started: frame.executionStarted };
  return { kind: "result", text: sourceText(frame.content.filter((part) => part.type === "text").map((part) => part.text).join("\n")), partial: frame.partial, error: frame.error, durationMs: frame.durationMs, image: frame.content.some((part) => part.type === "image"), showImages: frame.showImages };
}
export function duration(value: Outcome, expanded = false): Segment[] {
  return value.kind === "result" && !value.partial && value.durationMs !== undefined && Number.isFinite(value.durationMs) && (expanded || value.durationMs >= 1000)
    ? [{ ...segment(`${(Math.max(0, value.durationMs) / 1000).toFixed(1)}s`, "dim", 0), shrink: "omit", separatorBefore: "  " }] : [];
}
export function errorNotice(value: Extract<Outcome, { kind: "result" }>): Notice {
  return { text: labelText(value.text.split("\n").find((line) => line.trim()) || "Tool failed"), severity: "error" };
}
export function activityState(value: Outcome): ActivityState {
  return value.kind === "call" ? value.started ? "running" : value.argsComplete ? "queued" : "receiving"
    : value.partial ? "running" : value.error ? "failed" : "done";
}
