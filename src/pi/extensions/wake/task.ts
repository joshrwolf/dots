import {Type, type Static} from 'typebox';
import type {TaskEvent} from '../../src/host/tasks.ts';
import {untrusted, type Transition} from './event.ts';

// The task trigger's rules: which task store events match a wait or end it.
// Pure; the feed decides which events belong to the watched tree, and the
// wake engine owns effects.

/** Statuses after which a task records nothing more unless reopened. */
export const CLOSED: readonly string[] = ['done', 'dropped'];

export const TaskTriggerSchema = Type.Object(
  {
    kind: Type.Literal('task'),
    /** The watched root; its descendants count too. */
    id: Type.String({pattern: '^t-[a-f0-9]+$', maxLength: 128}),
    /** The registered project key, resolved when the wake was set. */
    project: Type.String({minLength: 1, maxLength: 256}),
    /** The root's title when the wake was set; writer-controlled text. */
    title: Type.String({maxLength: 512}),
    /** Event kinds, validated against the CLI's vocabulary when the wake was set. */
    until: Type.Array(Type.String({minLength: 1, maxLength: 64}), {
      minItems: 1,
      uniqueItems: true,
    }),
    /** Give up at this time, epoch ms. */
    deadline: Type.Integer({minimum: 0}),
    /** The seq of the last project event this wake has seen. */
    cursor: Type.Integer({minimum: 0}),
  },
  {additionalProperties: false},
);
export type TaskTrigger = Static<typeof TaskTriggerSchema>;

export interface TaskTransition extends Transition {
  readonly event?: TaskEvent;
}

export interface TaskStep {
  readonly transitions: readonly TaskTransition[];
  /** This session closed the root itself: finish without waking it. */
  readonly close?: boolean;
}

const NOTHING: TaskStep = {transitions: []};

/**
 * One event's consequence for a wait. `inTree` says whether the event's task
 * is the root or one of its descendants; `own` whether this session wrote it.
 */
export function observeTask(
  trigger: TaskTrigger,
  event: TaskEvent,
  {inTree, own}: {readonly inTree: boolean; readonly own: boolean},
): TaskStep {
  if (!inTree) return NOTHING;
  const root = event.task === trigger.id;
  const closed = root && CLOSED.includes(event.kind);
  if (own) return closed ? {transitions: [], close: true} : NOTHING;
  const where = root ? 'the task' : `descendant ${event.task}`;
  if (trigger.until.includes(event.kind))
    return {
      transitions: [
        {
          outcome: 'matched',
          summary: `${event.kind} on ${where}`,
          event,
          final: false,
        },
      ],
    };
  if (closed)
    return {
      transitions: [
        {
          outcome: 'ended',
          summary: `the task was ${event.kind === 'done' ? 'closed as done' : 'dropped'}, so no waited-for event can follow`,
          event,
          final: true,
        },
      ],
    };
  return NOTHING;
}

/** One observed event as a line; quoted values were written by task store writers. */
export function eventLine(event: TaskEvent): string {
  return [
    `#${event.seq} ${event.kind} on ${event.task}`,
    event.actor === null ? '' : `by ${untrusted(event.actor, 128)}`,
    event.detail === null ? '' : `detail ${untrusted(event.detail)}`,
    event.result === null ? '' : `result ${untrusted(event.result, 600)}`,
  ]
    .filter(Boolean)
    .join(' · ');
}
