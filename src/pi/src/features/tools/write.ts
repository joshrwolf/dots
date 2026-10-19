import { displayLimits } from "../../ui/policy.ts";
import type { Theme } from "@earendil-works/pi-coding-agent";
import type { PresentationView, ToolObservation, ToolPresentation } from "../../host/tools.ts";
import { activityHeader, OutputView, ActivityCard, type Notice } from "../../ui/card.ts";
import { SourceView } from "../../ui/previews/view.ts";
import type { SyntaxService } from "../../ui/previews/syntax.ts";
import { activityState, duration, errorNotice, pathLabel, lines, outcome, record, segment, string, type Outcome } from "./records.ts";

interface WriteModel { readonly path: string; readonly subject: string; readonly source: string | undefined; readonly outcome: Outcome }
export function projectWrite(frame: ToolObservation): WriteModel {
  const args = record(frame.args);
  const path = string(args?.path) ?? "…";
  return { path, subject: pathLabel(path, frame.cwd), source: string(args?.content), outcome: outcome(frame) };
}
class WriteView implements PresentationView<WriteModel> {
  private readonly card = new ActivityCard();
  private preview: SourceView | undefined;
  private source: string | undefined;
  private path = "";
  private count = 0;
  private readonly syntax: SyntaxService;
  private readonly notify: () => void;
  constructor(syntax: SyntaxService, notify: () => void) { this.syntax = syntax; this.notify = notify; }
  update(model: WriteModel, theme: Theme, expanded: boolean): void {
    const { outcome: state } = model;
    const segments = [];
    const notices: Notice[] = [];
    let body: OutputView | SourceView | undefined;
    if (state.kind === "call") {
      const chars = model.source?.length ?? 0;
      const count = chars >= 1000 ? `${(chars / 1000).toFixed(1)}k` : String(chars);
      if (!state.argsComplete && chars) segments.push(segment(`~${count} chars`));
    } else if (!state.partial && state.error) {
        notices.push(errorNotice(state));
      if (expanded) body = new OutputView(state.text);
    } else if (!state.partial) {
      if (model.source !== this.source || model.path !== this.path) {
        this.source = model.source;
        this.path = model.path;
        this.count = lines(model.source ?? "");
        this.preview = undefined;
      }
      if (model.source !== undefined) segments.push(segment(`${this.count} ${this.count === 1 ? "line" : "lines"}`));
      if (model.source === undefined) notices.push({ text: "Source preview unavailable; recorded output follows.", severity: "dim" });
      if ((model.source?.length ?? 0) > displayLimits.sourceCodeUnits) notices.push({ text: "Source exceeds display limit; submitted content is retained in the recorded call.", severity: "warning" });
      if (expanded) {
        if (model.source === undefined) body = new OutputView(state.text);
        else {
          this.preview ??= new SourceView({ source: model.source.slice(0, displayLimits.sourceCodeUnits), path: model.path, theme, syntax: this.syntax, invalidate: this.notify });
          this.preview.setTheme(theme);
          body = this.preview;
        }
      }
    }
    segments.push(...duration(state, expanded));
    this.card.update({ state: activityState(state), header: activityHeader("write", model.subject, segments), notices, body, caption: model.path, captionPolicy: "when-not-in-header" }, theme);
  }
  invalidate(): void { this.card.invalidate(); }
  render(width: number): string[] { return this.card.render(width); }
}
export function writePresentation(syntax: SyntaxService): ToolPresentation<WriteModel> {
  return { shell: "self", project: projectWrite, create: (notify) => new WriteView(syntax, notify) };
}
