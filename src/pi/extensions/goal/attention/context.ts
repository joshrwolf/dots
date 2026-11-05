import {sliceByColumn} from '@earendil-works/pi-tui';
import {labelText, sourceText} from '@joshrwolf/pi-kit/ui';
import {answerText} from './model.ts';
import type {Decision, GoalAttention, GoalRef, Request} from './source.ts';

/** Newest decisions shown besides the mandate; the rest are counted. */
export const DECISION_PREVIEW = 5;
const MANDATE_CHARS = 8192;
const PREVIEW_CHARS = 480;
const ANSWER_CHARS = 2048;
const REQUESTS_SHOWN = 8;

/** What the attention section of one goal is built from: one bounded read. */
export interface AttentionFacts {
  readonly goal: GoalAttention | undefined;
  /** Every recorded decision, oldest first; undefined when unread. */
  readonly decisions: readonly Decision[] | undefined;
  /** The request a request-linked mandate came from, resolved on demand. */
  readonly mandateRequest: Request | undefined;
  readonly failure: string | undefined;
}

const clip = (value: string, max: number) => {
  const chars = [...sourceText(value)];
  return chars.length > max
    ? `${chars.slice(0, max).join('')}…`
    : chars.join('');
};
const line = (value: string, max = 160) =>
  sliceByColumn(labelText(value), 0, max, true);

/** The human input a decision rests on, kept separate from its wording. */
function humanBasis(
  decision: Decision,
  request: Request | undefined,
): string[] {
  if (decision.human)
    return [
      `Human input (${decision.human.source}, ${line(decision.human.provenance, 120)}): ${clip(decision.human.text, ANSWER_CHARS)}`,
    ];
  if (decision.request && request?.answer)
    return [
      `From request ${request.id}. Question: ${clip(request.question, ANSWER_CHARS)}`,
      `Answer (${request.answer.source}, ${line(request.answer.provenance, 120)}): ${clip(answerText(request), ANSWER_CHARS)}`,
    ];
  if (decision.request)
    return [
      `From request ${decision.request}; its question and answer are not loaded: dots attention show ${decision.request}`,
    ];
  return ['No recorded human input; do not treat this wording as consent.'];
}

function role(goal: GoalAttention, owner: string | undefined): string {
  if (goal.status === 'held')
    return 'The goal is held (paused): do not dispatch or resume work, whatever answers or prompts arrive, until a person resumes it.';
  if (owner && goal.coordinator === owner)
    return 'This session coordinates the goal: incorporate saved answers with `dots attention incorporate R --content-file F` before continuing the work they hold.';
  return goal.coordinator
    ? `Coordinated by ${line(goal.coordinator, 80)}, not this session: do not incorporate answers or dispatch work for this goal.`
    : 'No session coordinates this goal: do not incorporate answers or dispatch its work.';
}

/**
 * The durable agreement and outstanding human requests of one goal, from the
 * store rather than the transcript, so they survive compaction, tree moves
 * and handoff. Deterministic for the same records; bounded, with omission
 * counts and the command that retrieves the rest.
 */
export function attentionContext(
  ref: GoalRef,
  facts: AttentionFacts,
  owner: string | undefined,
  followed = true,
): string {
  const scope = `--scope goals --goal ${ref.project}:${ref.id}`;
  const identity = followed
    ? `Goal ${ref.id} · project ${ref.project} · attention and decisions`
    : `Goal ${ref.id} · project ${ref.project} · coordinating, not followed here`;
  const unfollowed = followed
    ? []
    : [
        `This session coordinates this goal without following it, so its brief and tasks are not in context; following is only the view, and coordination continues until its root claim is released. Before dispatching its work, read them with goal_focus {"id": "${ref.id}", "project": "${ref.project}"} or dots task goal show ${ref.id} --project ${ref.project}.`,
      ];
  if (facts.failure || !facts.goal)
    return [
      identity,
      ...unfollowed,
      `Unavailable: ${line(facts.failure ?? 'not loaded', 512)}. Read with dots attention list ${scope} before relying on the mandate or answers.`,
    ].join('\n');
  const {goal} = facts;
  const lines = [identity, ...unfollowed, role(goal, owner)];
  const mandate = goal.mandate;
  if (mandate)
    lines.push(
      `Current mandate ${mandate.id}:`,
      clip(mandate.decision, MANDATE_CHARS),
      ...humanBasis(mandate, facts.mandateRequest),
    );
  else lines.push('No mandate recorded.');
  const decisions = (facts.decisions ?? []).filter(
    item => item.kind === 'decision',
  );
  if (facts.decisions === undefined)
    lines.push(`Decisions not loaded: dots attention decisions ${scope}`);
  else if (decisions.length) {
    const recent = decisions.slice(-DECISION_PREVIEW).reverse();
    lines.push('Recent decisions, newest first:');
    for (const decision of recent)
      lines.push(
        `- ${decision.id}: ${clip(decision.decision, PREVIEW_CHARS)}${decision.request ? ` (from request ${decision.request})` : decision.human ? ` (human input: ${decision.human.source})` : ''}`,
      );
    if (decisions.length > recent.length)
      lines.push(
        `${decisions.length - recent.length} earlier decisions not shown.`,
      );
    lines.push(
      `All decisions and their human input: dots attention decisions ${scope}`,
    );
  }
  const requests = [...goal.requests].sort(
    (a, b) => a.created_at - b.created_at || a.id.localeCompare(b.id),
  );
  const answered = requests.filter(request => request.state === 'answered');
  const open = requests.filter(request => request.state === 'open');
  for (const request of answered.slice(0, REQUESTS_SHOWN))
    lines.push(
      `Saved answer, not yet incorporated — ${request.id}: ${line(request.title)}`,
      `Question: ${clip(request.question, ANSWER_CHARS)}`,
      `Answer (${request.answer ? `${request.answer.source}, ${line(request.answer.provenance, 120)}` : 'not recorded'}): ${clip(answerText(request), ANSWER_CHARS)}`,
      ...(request.answer?.interpretation
        ? [
            `Recorded interpretation: ${clip(request.answer.interpretation, PREVIEW_CHARS)}`,
          ]
        : []),
    );
  for (const request of open.slice(0, REQUESTS_SHOWN))
    lines.push(
      `Awaiting a person — ${request.id}: ${line(request.title)}${request.tasks.length ? ` (holds ${request.tasks.map(task => task.id).join(', ')})` : ''}`,
    );
  const omitted =
    Math.max(0, answered.length - REQUESTS_SHOWN) +
    Math.max(0, open.length - REQUESTS_SHOWN);
  if (omitted) lines.push(`${omitted} more requests not shown.`);
  if (open.length)
    lines.push(
      followed
        ? 'A person answers in the goal popup (Alt+G) or in chat. Never bind a chat message to a request without their words; record chat answers with attention_record_chat.'
        : "A person answers in Attention, outside this session's goal popup; recording a chat answer needs the goal followed first.",
    );
  lines.push(`Requests: dots attention list ${scope}`);
  return lines.join('\n');
}
