import {labelText} from '@joshrwolf/pi-kit/ui';
import type {AttentionList, GoalAttention, Request} from './source.ts';

/**
 * What this session knows about one request, beyond the stored record:
 * a save in flight, a failed save, or a record that changed while the person
 * had a draft. Stored state stays authoritative; local state never claims a
 * save that the store did not confirm.
 */
export type ItemState =
  | {readonly kind: 'open'}
  | {readonly kind: 'saving'}
  | {readonly kind: 'answered'}
  /** Replaced by a new question; the draft stays with the original. */
  | {readonly kind: 'stale'; readonly replacement: Request | undefined}
  /** Closed or answered elsewhere while a draft or save was pending. */
  | {readonly kind: 'closed'; readonly message: string}
  /** The store did not record the reply; the request is still open. */
  | {readonly kind: 'error'; readonly message: string};

export type Coordination = 'self' | 'other' | 'none';

export interface AttentionItem {
  readonly request: Request;
  /** The goal's title, for display. */
  readonly goal: string;
  readonly state: ItemState;
  readonly coordination: Coordination;
  /** The goal root holds as paused; nothing dispatches until it resumes. */
  readonly paused: boolean;
  readonly coordinator: string | null;
}

export interface AttentionModel {
  readonly items: readonly AttentionItem[];
  /** Goals or projects in scope that could not be read. */
  readonly unavailable: readonly string[];
}

/** Local outcomes that outlive a refresh until the person dismisses them. */
export type Local = ReadonlyMap<
  string,
  {readonly state: ItemState; readonly request: Request; readonly goal: string}
>;

/** Needs a human: open, not yet saved. */
export const needsAnswer = (item: AttentionItem): boolean =>
  item.state.kind === 'open' || item.state.kind === 'error';
/** Still on the goal: open or answered but not incorporated. */
export const isOutstanding = (item: AttentionItem): boolean =>
  item.state.kind !== 'stale' && item.state.kind !== 'closed';

export function answerText(request: Request): string {
  const answer = request.answer;
  if (!answer) return '';
  const choice = request.choices.find(item => item.id === answer.choice);
  return labelText(
    [choice?.label, answer.text].filter(Boolean).join(' · ') || '(no text)',
  );
}

export function coordinationNote(item: AttentionItem): string {
  if (item.paused)
    return 'The goal is paused; the answer waits until it is explicitly resumed.';
  if (item.coordination === 'self')
    return 'This session coordinates the goal and continues with your answer.';
  if (item.coordination === 'other')
    return `The goal's coordinator (${labelText(item.coordinator ?? '')}) incorporates it when it runs; a claim does not prove that session is running.`;
  return 'No session coordinates this goal; the answer waits until coordination resumes.';
}

function goalItems(
  goal: GoalAttention,
  owner: string | undefined,
  local: Local,
): AttentionItem[] {
  const coordination: Coordination = !goal.coordinator
    ? 'none'
    : goal.coordinator === owner
      ? 'self'
      : 'other';
  return goal.requests.map(request => {
    const kept = local.get(request.id)?.state;
    const state: ItemState =
      request.state === 'answered'
        ? {kind: 'answered'}
        : kept && (kept.kind === 'saving' || kept.kind === 'error')
          ? kept
          : {kind: 'open'};
    return {
      request,
      goal: goal.title,
      state,
      coordination,
      paused: goal.status === 'held',
      coordinator: goal.coordinator,
    };
  });
}

/**
 * Items in a stable order: requests that need an answer first, then saved
 * answers, then local outcomes. Within each, oldest first.
 */
export function attentionModel(
  list: AttentionList | undefined,
  owner: string | undefined,
  local: Local,
): AttentionModel {
  const items = (list?.goals ?? []).flatMap(goal =>
    goalItems(goal, owner, local),
  );
  const listed = new Set(items.map(item => item.request.id));
  for (const [id, kept] of local)
    if (
      !listed.has(id) &&
      (kept.state.kind === 'stale' || kept.state.kind === 'closed')
    )
      items.push({
        request: kept.request,
        goal: kept.goal,
        state: kept.state,
        coordination: 'none',
        paused: false,
        coordinator: null,
      });
  const rank = (item: AttentionItem) =>
    needsAnswer(item) || item.state.kind === 'saving'
      ? 0
      : item.state.kind === 'answered'
        ? 1
        : 2;
  items.sort(
    (a, b) =>
      rank(a) - rank(b) ||
      a.request.created_at - b.request.created_at ||
      a.request.id.localeCompare(b.request.id),
  );
  return {
    items,
    unavailable: (list?.errors ?? []).map(
      error => `${error.goal ?? error.project}: ${error.error}`,
    ),
  };
}

/** The quiet near-editor line: the actual need, with a count of the others. */
export function indicator(model: AttentionModel): string | undefined {
  const waiting = model.items.filter(needsAnswer);
  const first = waiting[0];
  if (!first) return undefined;
  return `Needs your attention: ${labelText(first.request.title)}${waiting.length > 1 ? ` (+${waiting.length - 1} more)` : ''}`;
}
