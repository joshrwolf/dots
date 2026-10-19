import type { Theme } from "@earendil-works/pi-coding-agent";
import type { PresentationView, ToolObservation, ToolPresentation } from "../../host/tools.ts";
import { activityHeader, OutputView, ActivityCard, type Notice } from "../../ui/card.ts";
import { parsePatch, type Patch } from "../../ui/previews/patch.ts";
import { DiffView } from "../../ui/previews/view.ts";
import type { SyntaxService } from "../../ui/previews/syntax.ts";
import { activityState, duration, errorNotice, pathLabel, outcome, record, segment, string, type Outcome } from "./records.ts";

interface EditModel { readonly path: string; readonly subject: string; readonly patch: string | undefined; readonly fallback: string; readonly outcome: Outcome }
export function projectEdit(frame: ToolObservation): EditModel {
  const details = frame.slot === "result" ? record(frame.details) : undefined;
  const state = outcome(frame);
  const path = string(record(frame.args)?.path) ?? "…";
  return { path, subject: pathLabel(path, frame.cwd), patch: string(details?.patch), fallback: string(details?.diff) ?? string(details?.patch) ?? (state.kind === "result" ? state.text : ""), outcome: state };
}
class EditView implements PresentationView<EditModel> {
  private readonly card = new ActivityCard();
  private raw: string | undefined;
  private path = "";
  private patch: Patch | undefined;
  private preview: DiffView | undefined;
  private readonly syntax: SyntaxService;
  private readonly notify: () => void;
  constructor(syntax: SyntaxService, notify: () => void) { this.syntax = syntax; this.notify = notify; }
  update(model: EditModel, theme: Theme, expanded: boolean): void {
    const segments = [];
    const state = model.outcome;
    const notices: Notice[] = [];
    let body: OutputView | DiffView | undefined;
    if (state.kind === "result" && !state.partial) {
      if (state.error) {
        notices.push(errorNotice(state));
        if (expanded) body = new OutputView(state.text);
      } else {
        if (model.patch !== this.raw || model.path !== this.path) {
          this.raw = model.patch;
          this.path = model.path;
          this.patch = model.patch === undefined ? undefined : parsePatch(model.patch);
          this.preview = undefined;
        }
        if (this.patch) {
          segments.push(segment(`+${this.patch.added}`, "toolDiffAdded", 100), { ...segment(`−${this.patch.removed}`, "toolDiffRemoved", 100), separatorBefore: " " });
          if (expanded) {
            this.preview ??= new DiffView({ patch: this.patch, path: model.path, theme, syntax: this.syntax, invalidate: this.notify });
            this.preview.setTheme(theme);
            body = this.preview;
          }
        } else {
          segments.push(segment("diff unavailable"));
          if (expanded) body = new OutputView(`Diff preview unavailable; recorded output follows.\n${model.fallback}`);
        }
      }
    }
    segments.push(...duration(state, expanded));
    this.card.update({ state: activityState(state), header: activityHeader("edit", model.subject, segments), notices, body, caption: model.path, captionPolicy: "when-not-in-header" }, theme);
  }
  invalidate(): void { this.card.invalidate(); }
  render(width: number): string[] { return this.card.render(width); }
}
export function editPresentation(syntax: SyntaxService): ToolPresentation<EditModel> {
  return { shell: "self", project: projectEdit, create: (notify) => new EditView(syntax, notify) };
}
