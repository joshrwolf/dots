import {labelText} from '@joshrwolf/pi-kit/ui';
import type {AttentionList, Request} from './source.ts';

export type Coordination = 'self' | 'other' | 'none';
export interface AttentionItem {
  readonly request: Request;
  readonly goal: string;
  readonly state: {readonly kind: 'open' | 'answered'};
  readonly coordination: Coordination;
  readonly paused: boolean;
  readonly coordinator: string | null;
}
export interface AttentionModel {
  readonly items: readonly AttentionItem[];
  readonly unavailable: readonly string[];
}
export const needsAnswer = (item: AttentionItem): boolean =>
  item.state.kind === 'open';

export function answerText(request: Request): string {
  const answer = request.answer;
  if (!answer) return '';
  const choice = request.choices.find(item => item.id === answer.choice);
  return labelText(
    [choice?.label, answer.text].filter(Boolean).join(' · ') || '(no text)',
  );
}

/** Canonical outstanding requests in this runtime's exact active/followed scope. */
export function attentionModel(
  list: AttentionList | undefined,
  owner: string | undefined,
): AttentionModel {
  const items: AttentionItem[] = (list?.goals ?? []).flatMap(goal =>
    goal.requests
      .filter(request => request.state !== 'closed')
      .map(request => ({
        request,
        goal: goal.title,
        state: {
          kind:
            request.state === 'answered'
              ? ('answered' as const)
              : ('open' as const),
        },
        coordination: !goal.coordinator
          ? ('none' as const)
          : goal.coordinator === owner
            ? ('self' as const)
            : ('other' as const),
        paused: goal.status === 'held',
        coordinator: goal.coordinator,
      })),
  );
  items.sort(
    (a, b) =>
      Number(needsAnswer(b)) - Number(needsAnswer(a)) ||
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
