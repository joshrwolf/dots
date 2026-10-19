import type { Theme } from "@earendil-works/pi-coding-agent";
import { VStack, type Component } from "@earendil-works/pi-tui";
import type { PresentationView, ToolObservation, ToolPresentation } from "../../host/tools.ts";
import { Notices, OutputView, ToolCard, type Notice } from "../../ui/card.ts";
import { displayLimits } from "../../ui/policy.ts";
import { StateGutter } from "../../ui/state.ts";
import { PriorityRow } from "../../ui/row.ts";
import { plainLines } from "../../ui/text.ts";
import { SourceView } from "../../ui/previews/view.ts";
import type { SyntaxService } from "../../ui/previews/syntax.ts";
import { activityState, duration, outcome, record, segment, string, type Outcome } from "./records.ts";

const CALL_LIMIT = 256;
const statuses = ["running", "ok", "error", "cancelled"] as const;
type Status = typeof statuses[number];
interface Call { readonly name: string; readonly args: string; readonly status: Status; readonly error: string; readonly durationMs?: number; readonly cost?: number }
interface CodemodeModel {
  readonly code: string; readonly calls: readonly Call[]; readonly total: number;
  readonly available: boolean; readonly unreadable: number; readonly fullOutput: string;
  readonly output: string; readonly outcome: Outcome;
}
const finite = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value) && value >= 0;
const bounded = (value: string, limit = 2000): string => value.length <= limit ? value : `${value.slice(0, limit)}… [display limited]`;

/** Only the codemode result's recorded summaries; never parse abbreviated JSON as real arguments. */
export function projectCodemode(frame: ToolObservation): CodemodeModel {
  const details = frame.slot === "result" ? record(frame.details) : undefined;
  const raw: unknown[] | undefined = Array.isArray(details?.calls) ? details.calls : undefined;
  const calls: Call[] = [];
  let unreadable = 0;
  for (const value of (raw ?? []).slice(0, CALL_LIMIT)) {
    const call = record(value);
    const status = statuses.find((status) => status === call?.status);
    if (!call || typeof call.name !== "string" || typeof call.args !== "string" || !status) { unreadable++; continue; }
    calls.push({ name: bounded(call.name, 200), args: bounded(call.args), status, error: bounded(string(call.error) ?? ""),
      ...(finite(call.durationMs) ? { durationMs: call.durationMs } : {}), ...(finite(call.cost) ? { cost: call.cost } : {}) });
  }
  // Remove only the exact native header block, not similarly-worded user output.
  const header = /^Script (completed|failed)\nWall time [\d.]+ seconds\nOutput:\n$/;
  const content = frame.slot === "result" ? frame.content : [];
  const first = content[0];
  const output = outcome(frame.slot === "result" ? { ...frame, content: first?.type === "text" && header.test(first.text) ? content.slice(1) : content } : frame);
  return { code: string(record(frame.args)?.code) ?? "", calls, total: raw?.length ?? 0, available: raw !== undefined, unreadable,
    fullOutput: bounded(string(details?.fullOutputPath) ?? ""), output: output.kind === "result" ? output.text : "", outcome: outcome(frame) };
}

const label = (status: Status): string => status === "ok" ? "done" : status === "error" ? "failed" : status;
const rank = (call: Call): number => call.status === "error" ? 0 : call.status === "running" ? 1 : call.status === "cancelled" ? 2 : 3;

/** Stable input order when expanded; collapsed rows prioritize failures and active calls. */
class CallList implements Component {
  private readonly children: Component[] = [];
  private readonly omitted: number;
  private readonly expanded: boolean;
  constructor(calls: readonly Call[], theme: Theme, expanded: boolean) {
    this.expanded = expanded;
    const selected = expanded ? calls : [...calls].sort((a, b) => rank(a) - rank(b)).slice(0, 4);
    this.omitted = calls.length - selected.length;
    for (const call of selected) {
      const row = new PriorityRow();
      row.update({ layout: "inline", separator: "  ", groupSeparator: "  ", endSeparator: " ", start: [
        { ...segment(call.name, "toolTitle", 90), bold: true, padTo: 5 }, segment(call.args, "dim", 10),
      ], end: [
        ...(call.durationMs === undefined || (!expanded && call.durationMs < 1000) ? [] : [{ ...segment(`${(call.durationMs / 1000).toFixed(1)}s`, "dim", 0), separatorBefore: " ", shrink: "omit" as const }]),
        ...(call.cost === undefined ? [] : [{ ...segment(`$${call.cost.toPrecision(3)}`, "dim", 0), shrink: "omit" as const }]),
      ] }, theme);
      this.children.push(new StateGutter(row, call.status === "ok" ? "done" : call.status === "error" ? "failed" : call.status, theme));
      if (expanded) {
        const notice = new Notices();
        notice.update([
          { text: `${call.name} · ${label(call.status)}${call.durationMs === undefined ? "" : ` · ${(call.durationMs / 1000).toFixed(1)}s`}${call.cost === undefined ? "" : ` · $${call.cost.toPrecision(3)}`}`, severity: "dim" },
          ...(call.args ? [{ text: `Arguments (preview): ${call.args}`, severity: "dim" as const }] : []),
          ...(call.error ? [{ text: call.error, severity: "error" as const }] : []),
        ], theme);
        this.children.push(notice);
      }
    }
  }
  invalidate(): void { for (const child of this.children) child.invalidate(); }
  render(width: number): string[] {
    if (width <= 0) return [];
    const lines: string[] = [];
    const budget = this.expanded ? displayLimits.previewRows : 5;
    for (const child of this.children) {
      lines.push(...child.render(width));
      if (lines.length >= budget) return [...lines.slice(0, budget - 1), ...plainLines("… call display limited", width, 1)];
    }
    if (this.omitted) lines.push(...plainLines(`… ${this.omitted} more calls; expand to inspect`, width, 1));
    return lines;
  }
}

class CodemodeView implements PresentationView<CodemodeModel> {
  private readonly card = new ToolCard();
  private readonly syntax: SyntaxService;
  private readonly notify: () => void;
  private code = "";
  private preview: SourceView | undefined;
  constructor(syntax: SyntaxService, notify: () => void) { this.syntax = syntax; this.notify = notify; }
  update(model: CodemodeModel, theme: Theme, expanded: boolean): void {
    const state = model.outcome;
    const notices: Notice[] = [];
    if (state.kind === "result" && !model.available) notices.push({ text: "Nested call summaries unavailable in this result.", severity: "dim" });
    if (model.unreadable) notices.push({ text: `${model.unreadable} call records could not be displayed.`, severity: "warning" });
    if (model.total > CALL_LIMIT) notices.push({ text: `Call display limited to the first ${CALL_LIMIT} of ${model.total} records; counts below cover this sample only.`, severity: "warning" });
    if (model.fullOutput) notices.push({ text: `Full output: ${model.fullOutput}`, severity: "warning" });
    const failed = model.calls.filter((call) => call.status === "error");
    for (const call of failed.slice(0, 3)) notices.push({ text: `${call.name}: ${bounded(call.error || "failed (no error text recorded)", 500)}`, severity: "error" });
    if (failed.length > 3) notices.push({ text: `${failed.length - 3} more failed calls; expand to inspect.`, severity: "error" });
    if (state.kind === "result" && state.error) notices.push({ text: `Failure output tail: ${model.output.slice(-600) || "No diagnostic recorded."}`, severity: "error" });
    if (model.code.length > displayLimits.sourceCodeUnits) notices.push({ text: "Script display limited; recorded arguments retain the complete script.", severity: "warning" });
    const body = new VStack();
    const counts = statuses.map((status) => [status, model.calls.filter((call) => call.status === status).length] as const)
      .filter(([, count]) => count).map(([status, count]) => `${count} ${label(status)}`).join(" · ");
    const priced = model.calls.filter((call) => call.cost !== undefined);
    const cost = priced.reduce((total, call) => total + (call.cost ?? 0), 0);
    const summaryText = [counts || (model.available && model.total === 0 ? "0 calls" : ""),
      ...(priced.length ? [Number.isFinite(cost) ? `reported cost $${cost.toPrecision(3)}` : "reported cost exceeds display range"] : []),
    ].filter(Boolean).join(" · ");
    if (summaryText) {
      const summary = new Notices(); summary.update([{ text: summaryText, severity: "dim" }], theme); body.addChild(summary);
    }
    if (model.calls.length) body.addChild(new CallList(model.calls, theme, expanded));
    if (expanded && model.available) {
      const notice = new Notices(); notice.update([{ text: "Recorded summaries only: arguments/errors may be abbreviated; nested results are not recorded.", severity: "dim" }], theme); body.addChild(notice);
    }
    if (model.code !== this.code) { this.code = model.code; this.preview = undefined; }
    if (expanded && model.code) {
      this.preview ??= new SourceView({ source: model.code.slice(0, displayLimits.sourceCodeUnits), path: "script.js", theme, syntax: this.syntax, invalidate: this.notify });
      this.preview.setTheme(theme); body.addChild(this.preview);
    }
    if (model.output) {
      const output = new OutputView(model.output);
      body.addChild(expanded ? output : { invalidate: () => output.invalidate(), render: (width) => {
        const lines = output.render(width);
        return lines.length <= 4 ? lines : [...lines.slice(0, 3), ...plainLines("… expand for output", width, 1)];
      } });
    }
    this.card.update({ ...(state.kind === "result" ? { state: activityState(state) } : {}),
      groupSeparator: "  ", noticeIndent: 2, heading: [{ ...segment("codemode", "toolTitle", 90), bold: true }],
      status: duration(state, expanded),
      notices, body: model.calls.length || model.output || model.available || (expanded && model.code) ? body : undefined, caption: expanded ? "calls / recorded script / output" : "activity / output" }, theme);
  }
  invalidate(): void { this.card.invalidate(); }
  render(width: number): string[] { return this.card.render(width); }
}
export function codemodePresentation(syntax: SyntaxService): ToolPresentation<CodemodeModel> {
  return { shell: "self", project: projectCodemode, create: (notify) => new CodemodeView(syntax, notify) };
}
