import { displayLimits } from "../../ui/policy.ts";
import type { Theme } from "@earendil-works/pi-coding-agent";
import type { PresentationView, ToolObservation, ToolPresentation } from "../../host/tools.ts";
import { activityHeader, OutputView, ActivityCard, type Notice } from "../../ui/card.ts";
import { SourceView } from "../../ui/previews/view.ts";
import type { SyntaxService } from "../../ui/previews/syntax.ts";
import { activityState, duration, errorNotice, pathLabel, lines, outcome, positive, record, segment, string, type Outcome } from "./records.ts";

export function readExcerpt(text: string, args: Record<string, unknown>, details: unknown): { source: string; notice: string; start: number; count: number } {
  const start = positive(args.offset) ?? 1;
  const truncation = record(record(details)?.truncation);
  if (truncation?.firstLineExceedsLimit) return { source: "", notice: text, start, count: 0 };
  if (truncation?.truncated && typeof truncation.content === "string" && text.startsWith(truncation.content)) return { source: truncation.content, notice: text.slice(truncation.content.length).trim(), start, count: lines(truncation.content) };
  const limit = positive(args.limit);
  const suffix = /\n\n\[(\d+) more lines in file\. Use offset=(\d+) to continue\.\]$/.exec(text);
  if (limit && suffix && Number(suffix[2]) === start + limit) {
    const source = text.slice(0, suffix.index);
    if (source.split("\n").length === limit) return { source, notice: suffix[0].trim(), start, count: limit };
  }
  return { source: text, notice: "", start, count: lines(text) };
}
interface ReadModel { readonly path: string; readonly subject: string; readonly source: string; readonly start: number; readonly count: number; readonly notice: string; readonly truncated: boolean; readonly image: boolean; readonly outcome: Outcome }
export function projectRead(frame: ToolObservation): ReadModel {
  const args = record(frame.args) ?? {};
  const path = string(args.path) ?? "…";
  const state = outcome(frame);
  const text = frame.slot === "result" ? frame.content.filter((part) => part.type === "text").map((part) => part.text).join("\n") : "";
  const image = state.kind === "result" && (state.image || /\.(png|jpe?g|gif|webp|bmp)$/i.test(path) && /^Read image file \[/.test(text));
  const details = frame.slot === "result" ? frame.details : undefined;
  const excerpt = readExcerpt(text, args, details);
  return { path, subject: pathLabel(path, frame.cwd), ...excerpt, truncated: record(record(details)?.truncation)?.truncated === true, image, outcome: state };
}
class ReadView implements PresentationView<ReadModel> {
  private readonly card = new ActivityCard();
  private source = "";
  private path = "";
  private start = 0;
  private count = 0;
  private preview: SourceView | undefined;
  private readonly syntax: SyntaxService;
  private readonly notify: () => void;
  constructor(syntax: SyntaxService, notify: () => void) { this.syntax = syntax; this.notify = notify; }
  update(model: ReadModel, theme: Theme, expanded: boolean): void {
    const segments = [];
    const notices: Notice[] = [];
    const state = model.outcome;
    let body: OutputView | SourceView | undefined;
    if (state.kind === "result" && !state.partial) {
      if (state.error) {
        notices.push(errorNotice(state));
        if (expanded) body = new OutputView(state.text);
      } else if (model.image) {
        segments.push(segment(state.showImages ? "image" : "image · preview hidden"));
        // Keep image notes even when collapsed. Host still owns image blocks.
        body = new OutputView(state.text);
      } else {
        segments.push(segment(model.truncated && !model.source ? "read limit exceeded" : model.count ? `lines ${model.start}–${model.start + model.count - 1}` : "empty result"));
        if (model.truncated) segments.push(segment("truncated", "warning", 100));
        if (model.notice && (model.truncated || expanded)) notices.push({ text: model.notice, severity: model.truncated ? "warning" : "dim" });
        else if (model.notice) segments.push(segment("more available", "dim", 10));
        if (model.source.length > displayLimits.sourceCodeUnits) notices.push({ text: "Display limited; recorded result retains complete content.", severity: "warning" });
        if (model.source !== this.source || model.path !== this.path || model.start !== this.start || model.count !== this.count) {
          this.source = model.source; this.path = model.path; this.start = model.start; this.count = model.count; this.preview = undefined;
        }
        if (expanded) {
          this.preview ??= new SourceView({ source: model.source.slice(0, displayLimits.sourceCodeUnits), path: model.path, theme, syntax: this.syntax, invalidate: this.notify, startLine: model.start, preserveTrailingEmptyLine: model.count > lines(model.source) });
          this.preview.setTheme(theme);
          body = this.preview;
        }
      }
      segments.push(...duration(state, expanded));
    }
    this.card.update({ state: activityState(state), header: activityHeader("read", model.subject, segments), notices, body, caption: model.path, captionPolicy: "when-not-in-header" }, theme);
  }
  invalidate(): void { this.card.invalidate(); }
  render(width: number): string[] { return this.card.render(width); }
}
export function readPresentation(syntax: SyntaxService): ToolPresentation<ReadModel> {
  return { shell: "self", project: projectRead, create: (notify) => new ReadView(syntax, notify) };
}
