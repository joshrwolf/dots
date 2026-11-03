import {Type, type Static} from 'typebox';
import type {
  ExitStatus,
  PaneSnapshot,
  RexEvent,
  StatusRecord,
} from '../../src/host/rex.ts';

// The pane trigger's rules: what counts as a match or an end, from live
// events and from snapshots after a gap. Pure; the wake engine owns effects.

export const PANE_UNTIL = ['exit', 'done', 'error', 'blocked', 'idle'] as const;
export type PaneUntil = (typeof PANE_UNTIL)[number];
export const PaneUntilSchema = Type.Union([
  Type.Literal('exit'),
  Type.Literal('done'),
  Type.Literal('error'),
  Type.Literal('blocked'),
  Type.Literal('idle'),
]);

/** The last root status record observed, enough to recognize a change after a gap. */
const ObservedSchema = Type.Object(
  {
    state: Type.String({maxLength: 64}),
    /** Rex's opaque timestamp string: compared for identity, never ordered. */
    updatedAt: Type.String({maxLength: 64}),
    owner: Type.Optional(Type.Integer()),
  },
  {additionalProperties: false},
);
export type Observed = Static<typeof ObservedSchema>;

export const PaneTriggerSchema = Type.Object(
  {
    kind: Type.Literal('pane'),
    /** The target as given, for display. */
    target: Type.String({minLength: 1, maxLength: 256}),
    host: Type.Optional(Type.String({minLength: 1, maxLength: 256})),
    session: Type.String({minLength: 1, maxLength: 256}),
    sessionLabel: Type.String({maxLength: 256}),
    block: Type.String({minLength: 1, maxLength: 256}),
    label: Type.String({maxLength: 256}),
    until: Type.Array(PaneUntilSchema, {minItems: 1, uniqueItems: true}),
    /** Give up at this time, epoch ms. */
    deadline: Type.Integer({minimum: 0}),
    /** The Rex server run the block belongs to. */
    instance: Type.String({minLength: 1, maxLength: 256}),
    record: Type.Optional(ObservedSchema),
  },
  {additionalProperties: false},
);
export type PaneTrigger = Static<typeof PaneTriggerSchema>;

export type Outcome = 'matched' | 'ended' | 'timed out' | 'manual';

export interface Transition {
  readonly outcome: Outcome;
  /** One line in our own words; program text never appears here. */
  readonly summary: string;
  readonly state?: string;
  readonly exit?: ExitStatus;
  /** The pane can produce nothing more: the wake finishes with this fire. */
  readonly final: boolean;
}

export interface Step {
  /** The root record to keep: a value, null once removed, absent when unchanged. */
  readonly record?: Observed | null;
  readonly transitions: readonly Transition[];
}

export function observed(record: StatusRecord): Observed {
  return {
    state: record.state,
    updatedAt: record.updatedAt,
    ...(record.owner ? {owner: record.owner.pid} : {}),
  };
}

const watchesExit = (trigger: PaneTrigger) => trigger.until.includes('exit');
const statusStates = (trigger: PaneTrigger): readonly string[] =>
  trigger.until.filter(state => state !== 'exit');
/**
 * Whether a status record's disappearance or a new owner ends the wait. A wake
 * that also waits for `exit` keeps waiting: a program clears its status
 * before it exits, and the exit code is the better answer.
 */
const statusEnds = (trigger: PaneTrigger) =>
  statusStates(trigger).length > 0 && !watchesExit(trigger);

const exited = (exit: ExitStatus): string =>
  `exited with code ${exit.code}${exit.error ? ` (${untrusted(exit.error, 120)})` : ''}`;

function exitTransition(trigger: PaneTrigger, exit: ExitStatus): Transition {
  return watchesExit(trigger)
    ? {
        outcome: 'matched',
        summary: `the block's program ${exited(exit)}`,
        exit,
        final: true,
      }
    : {
        outcome: 'ended',
        summary: `the block's program ${exited(exit)}`,
        exit,
        final: true,
      };
}

const ended = (summary: string): Transition => ({
  outcome: 'ended',
  summary,
  final: true,
});

function statusStep(
  trigger: PaneTrigger,
  current: Observed | undefined,
  live: boolean,
): Step {
  const previous = trigger.record;
  const states = statusStates(trigger);
  if (!current) {
    if (!previous) return {transitions: []};
    return {
      record: null,
      transitions: statusEnds(trigger)
        ? [
            ended(
              'the program stopped reporting status without reaching a waited-for state',
            ),
          ]
        : [],
    };
  }
  // Rex omits the owner when the reporting process exited before the report
  // was read, as when a program reports done right before it exits. An
  // unknown owner is not a different program.
  const ownerChanged =
    previous?.owner !== undefined &&
    current.owner !== undefined &&
    previous.owner !== current.owner;
  const matches = states.includes(current.state);
  // Live, a match is entry into the set (a re-report or progress update is
  // not). After a gap, any change of record identity in the set counts.
  const fire = live
    ? matches && (!previous || !states.includes(previous.state) || ownerChanged)
    : matches &&
      (!previous || previous.updatedAt !== current.updatedAt || ownerChanged);
  const transitions: Transition[] = fire
    ? [
        {
          outcome: 'matched',
          summary: `status ${current.state}${
            previous && previous.state !== current.state
              ? ` (was ${previous.state})`
              : ''
          }${ownerChanged ? ' from a different program' : ''}`,
          state: current.state,
          final: false,
        },
      ]
    : ownerChanged && statusEnds(trigger)
      ? [ended(`a different program now reports status ${current.state}`)]
      : [];
  return {record: current, transitions};
}

/** A live event's consequence for this pane, or undefined when it is about something else. */
export function observe(
  trigger: PaneTrigger,
  event: RexEvent,
): Step | undefined {
  if (event.session !== trigger.session) return undefined;
  if (event.type === 'session_destroyed')
    return {transitions: [ended('the Rex session was destroyed')]};
  if (event.block !== trigger.block) return undefined;
  switch (event.type) {
    case 'block_closed':
      return {transitions: [ended('the block was closed')]};
    case 'child_exited':
      return {transitions: [exitTransition(trigger, event.exit)]};
    case 'program_status_changed':
      return event.id === ''
        ? statusStep(trigger, observed(event.record), true)
        : undefined;
    case 'program_status_removed':
      if (!event.ids.includes('')) return undefined;
      // Rex drops a running program's record just before it reports the
      // exit; the exit that follows decides, as it does after a gap.
      if (event.reason === 'child_exited')
        return {
          ...(trigger.record ? {record: null} : {}),
          transitions: [],
        };
      return statusStep(trigger, undefined, true);
  }
}

/** What changed while nothing was watching, judged from a snapshot. */
export function reconcile(trigger: PaneTrigger, snapshot: PaneSnapshot): Step {
  if (snapshot.instance !== trigger.instance)
    return {
      transitions: [ended('the Rex server restarted, so the program is gone')],
    };
  if (!snapshot.present)
    return {
      transitions: [
        ended(
          snapshot.reason === 'block closed'
            ? 'the block was closed'
            : 'the Rex session was destroyed',
        ),
      ],
    };
  const status = statusStep(
    trigger,
    snapshot.record ? observed(snapshot.record) : undefined,
    false,
  );
  // An exit outranks a status record that vanished with its program.
  const transitions = snapshot.exit
    ? [
        ...status.transitions.filter(t => t.outcome === 'matched'),
        exitTransition(trigger, snapshot.exit),
      ]
    : status.transitions;
  return {...('record' in status ? {record: status.record} : {}), transitions};
}

const INVISIBLE = /[\p{Cc}\u200b-\u200f\u202a-\u202e\u2060-\u2069\ufeff]/gu;

/** Program-controlled text as a bounded, quoted literal with controls and direction overrides removed. */
export function untrusted(text: string, max = 300): string {
  const clean = text.replace(INVISIBLE, '');
  return JSON.stringify(clean.length > max ? `${clean.slice(0, max)}…` : clean);
}

/** A status record as one line; quoted values come from the program. */
export function statusLine(record: StatusRecord | undefined): string {
  if (!record) return 'no status reported';
  return [
    record.state,
    record.kind ? `kind ${untrusted(record.kind, 32)}` : '',
    record.progress === undefined ? '' : `${record.progress}%`,
    record.app ? `app ${untrusted(record.app, 32)}` : '',
    record.title ? `title ${untrusted(record.title, 192)}` : '',
    record.msg ? `msg ${untrusted(record.msg)}` : '',
  ]
    .filter(Boolean)
    .join(' · ');
}

export const untilLabel = (until: readonly string[]) => until.join('|');

/** How the pane is named in rows and messages. */
export function paneName(
  trigger: Pick<PaneTrigger, 'label' | 'target' | 'block'>,
): string {
  return trigger.label || trigger.target || trigger.block.slice(-8);
}
