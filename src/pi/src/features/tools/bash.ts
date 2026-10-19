import { displayLimits } from "../../ui/policy.ts";
import type { Theme } from "@earendil-works/pi-coding-agent";
import { Spacer, VStack, type Component } from "@earendil-works/pi-tui";
import type { PresentationView, ToolObservation, ToolPresentation } from "../../host/tools.ts";
import { OutputView, ToolCard, type Notice } from "../../ui/card.ts";
import { SourceView } from "../../ui/previews/view.ts";
import type { SyntaxService } from "../../ui/previews/syntax.ts";
import { activityState, duration, toolHeader, outcome, record, segment, string, type Outcome } from "./records.ts";

interface BashModel { readonly command: string; readonly truncated: boolean; readonly fullOutput: string; readonly status: string; readonly outcome: Outcome }
export function projectBash(frame: ToolObservation): BashModel {
  const state = outcome(frame);
  const details = frame.slot === "result" ? record(frame.details) : undefined;
  let truncated = record(details?.truncation)?.truncated === true;
  let fullOutput = string(details?.fullOutputPath) ?? "";
  let status = state.kind === "call" ? state.started ? "running" : state.argsComplete ? "queued" : "receiving" : "";
  if (state.kind === "result") {
    if (state.partial) status = "running";
    else if (!state.error) status = "";
    else {
      const last = state.text.trimEnd().split("\n").at(-1) || "failed";
      const exit = /^Command exited with code (\d+)$/.exec(last);
      status = exit ? `exit ${exit[1]}` : last;
      const notice = /\n\n\[Showing (?:lines|last) [^\n]* Full output: ([^\n]+)\](?:\n\nCommand (?:exited with code \d+|aborted|timed out after [^\n]+|terminated without an exit code))?$/.exec(state.text);
      if (!fullOutput && notice?.[1]) { fullOutput = notice[1]; truncated = true; }
    }
  }
  return { command: string(record(frame.args)?.command) ?? "…", truncated, fullOutput, status, outcome: state };
}
class BashView implements PresentationView<BashModel> {
  private readonly card = new ToolCard();
  private command = "";
  private preview: SourceView | undefined;
  private text = "";
  private output: OutputView | undefined;
  private readonly syntax: SyntaxService;
  private readonly notify: () => void;
  constructor(syntax: SyntaxService, notify: () => void) { this.syntax = syntax; this.notify = notify; }
  update(model: BashModel, theme: Theme, expanded: boolean): void {
    const segments = [];
    const notices: Notice[] = [];
    const state = model.outcome;
    let body: Component | undefined;
    if (model.command !== this.command) { this.command = model.command; this.preview = undefined; }
    if (expanded && (state.kind === "result" || state.argsComplete || state.started)) {
      this.preview ??= new SourceView({ source: model.command.slice(0, displayLimits.sourceCodeUnits), path: "command.sh", theme, syntax: this.syntax, invalidate: this.notify });
      this.preview.setTheme(theme);
      body = this.preview;
    }
    if (state.kind === "result") {
      if (!state.partial && state.error) {
        notices.push({ text: model.status, severity: "error" });
      }
      segments.push(...duration(state, expanded));
      if (model.truncated) segments.push(segment("truncated", "warning", 100));
      if (model.fullOutput) notices.push({ text: `Full output: ${model.fullOutput}`, severity: "warning" });
      if (state.text.length > displayLimits.sourceCodeUnits || model.command.length > displayLimits.sourceCodeUnits) notices.push({ text: "Display limited; recorded call/result retains complete content.", severity: "warning" });
      if (state.text !== this.text) { this.text = state.text; this.output = undefined; }
      if (body && state.text) {
        this.output ??= new OutputView(state.text);
        body = new VStack([body, new Spacer(1), this.output]);
      }
    }
    this.card.update({ state: activityState(state), header: toolHeader("bash", model.command, segments, "end"), notices, noticeIndent: 9, body, caption: "command / output" }, theme);
  }
  invalidate(): void { this.card.invalidate(); }
  render(width: number): string[] { return this.card.render(width); }
}
export function bashPresentation(syntax: SyntaxService): ToolPresentation<BashModel> {
  return { shell: "self", project: projectBash, create: (notify) => new BashView(syntax, notify) };
}
