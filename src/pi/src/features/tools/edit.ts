import type {Theme} from '@earendil-works/pi-coding-agent';
import type {
  PresentationOptions,
  PresentationView,
  ToolObservation,
  ToolPresentation,
} from '../../host/tools.ts';
import {
  activityHeader,
  OutputView,
  ActivityCard,
  type Notice,
} from '../../ui/card.ts';
import {DiffView} from '../../ui/previews/view.ts';
import {
  activityState,
  duration,
  errorNotice,
  pathLabel,
  outcome,
  record,
  segment,
  string,
  type Outcome,
} from './records.ts';

interface EditModel {
  readonly path: string;
  readonly subject: string;
  readonly diff: string | undefined;
  readonly fallback: string;
  readonly outcome: Outcome;
}

/** Changed-line counts from Pi's recorded diff rows (`+12 text`, `-12 text`). */
export function diffCounts(diff: string): {added: number; removed: number} {
  let added = 0;
  let removed = 0;
  for (const line of diff.split('\n')) {
    if (/^\+\s*\d+ /.test(line)) added++;
    else if (/^-\s*\d+ /.test(line)) removed++;
  }
  return {added, removed};
}
export function projectEdit(frame: ToolObservation): EditModel {
  const details = frame.slot === 'result' ? record(frame.details) : undefined;
  const state = outcome(frame);
  const path = string(record(frame.args)?.path) ?? '…';
  return {
    path,
    subject: pathLabel(path, frame.cwd),
    diff: string(details?.diff),
    fallback:
      string(details?.patch) ?? (state.kind === 'result' ? state.text : ''),
    outcome: state,
  };
}
class EditView implements PresentationView<EditModel> {
  private readonly card = new ActivityCard();
  private diff: string | undefined;
  private counts: {added: number; removed: number} | undefined;
  private preview: DiffView | undefined;
  update(
    model: EditModel,
    theme: Theme,
    {expanded}: PresentationOptions,
  ): void {
    const segments = [];
    const state = model.outcome;
    const notices: Notice[] = [];
    let body: OutputView | DiffView | undefined;
    if (state.kind === 'result' && !state.partial) {
      if (state.error) {
        notices.push(errorNotice(state));
        if (expanded) body = new OutputView(state.text);
      } else {
        if (model.diff !== this.diff) {
          this.diff = model.diff;
          this.counts =
            model.diff === undefined ? undefined : diffCounts(model.diff);
          this.preview = undefined;
        }
        if (this.diff !== undefined && this.counts) {
          segments.push(
            segment(`+${this.counts.added}`, 'toolDiffAdded', 100),
            {
              ...segment(`−${this.counts.removed}`, 'toolDiffRemoved', 100),
              separatorBefore: ' ',
            },
          );
          if (expanded) {
            this.preview ??= new DiffView(this.diff, theme);
            this.preview.setTheme(theme);
            body = this.preview;
          }
        } else {
          segments.push(segment('diff unavailable'));
          if (expanded)
            body = new OutputView(
              `Diff preview unavailable; recorded output follows.\n${model.fallback}`,
            );
        }
      }
    }
    segments.push(...duration(state, expanded));
    this.card.update(
      {
        expanded,
        state: activityState(state),
        header: activityHeader('edit', model.subject, segments),
        notices,
        body,
        caption: model.path,
        captionPolicy: 'when-not-in-header',
      },
      theme,
    );
  }
  invalidate(): void {
    this.card.invalidate();
  }
  render(width: number): string[] {
    return this.card.render(width);
  }
}
export const editPresentation: ToolPresentation<EditModel> = {
  shell: 'self',
  project: projectEdit,
  create: () => new EditView(),
};
