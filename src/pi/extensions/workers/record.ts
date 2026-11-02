import {Type, type Static, type TSchema} from 'typebox';
import {Check, Errors} from 'typebox/value';

// Persisted worker records: what a later runtime of the owning session needs to
// show or continue a worker. Runtime attachments (processes, questions,
// waiters) are never persisted. Records are session-scoped
// resources: every runtime folds the session's entries in file order, and
// only records stamped with the current session id are live. A fork or clone
// copies entries into a new session id, so its inherited records are history.

export const WORKER_ENTRY = 'dots.worker';
export const OUTBOX_ENTRY = 'dots.worker.outbox';
export const RECORD_VERSION = 1;

const text = (maxLength: number) => Type.String({maxLength});
const count = Type.Integer({minimum: 0});
const amount = Type.Number({minimum: 0});

export const StateSchema = Type.Union([
  Type.Literal('running'),
  Type.Literal('asking'),
  Type.Literal('completed'),
  Type.Literal('interrupted'),
  Type.Literal('unknown'),
  Type.Literal('failed'),
  Type.Literal('stopped'),
  Type.Literal('paused'),
]);
export type State = Static<typeof StateSchema>;
/** What a worker event reports: a state change, or a message, note, or routing failure. */
export type EventKind =
  | State
  | 'update'
  | 'message'
  | 'note'
  | 'undeliverable'
  | 'relay'
  | 'context-pressure';

export const AssignmentSchema = Type.Object(
  {
    project: Type.String({
      minLength: 1,
      maxLength: 4096,
      pattern: '^[^\\x00-\\x1f\\x7f-\\x9f]+$',
    }),
    id: Type.String({pattern: '^t-[a-f0-9]+$'}),
  },
  {additionalProperties: false},
);

export const SkillRefSchema = Type.Object(
  {name: Type.String({minLength: 1, maxLength: 256}), path: text(4096)},
  {additionalProperties: false},
);

export const WorkerRecordSchema = Type.Object(
  {
    version: Type.Literal(RECORD_VERSION),
    /** The session id that owns this worker's process and timers. */
    owner: Type.String({minLength: 1, maxLength: 256}),
    // Generated as w- and 16 hex digits; any id stays a safe path segment.
    id: Type.String({pattern: '^w-[a-z0-9-]{1,64}$'}),
    label: text(256),
    cwd: text(4096),
    sessionDir: text(4096),
    sessionFile: Type.Optional(text(4096)),
    model: Type.Optional(text(512)),
    thinking: Type.Optional(text(32)),
    tools: Type.Optional(Type.Array(text(256), {maxItems: 256})),
    system: Type.Optional(Type.String()),
    assignment: Type.Optional(AssignmentSchema),
    skills: Type.Optional(Type.Array(SkillRefSchema, {maxItems: 64})),
    state: StateSchema,
    startedAt: count,
    updatedAt: count,
    finishedAt: Type.Optional(count),
    calls: count,
    tokens: count,
    /** Pi's reported cost total for the worker's responses, in dollars. */
    cost: Type.Optional(amount),
    /** Set only when every counted response reported a provider-billed total; absent means an estimate. */
    costBilled: Type.Optional(Type.Literal(true)),
    /** Unknown (null) right after compaction until the next response reports usage. */
    contextTokens: Type.Optional(Type.Union([count, Type.Null()])),
    contextWindow: Type.Optional(amount),
    compactAt: Type.Optional(Type.Union([count, Type.Null()])),
    settlements: count,
    result: Type.Optional(Type.String()),
    partial: Type.Optional(Type.String()),
    error: Type.Optional(Type.String()),
  },
  {additionalProperties: false},
);
export type WorkerRecord = Static<typeof WorkerRecordSchema>;

/** The custom-message details a worker event carries; also frozen in completion journals. */
export const EventDetailsSchema = Type.Object({
  id: text(256),
  kind: text(64),
  label: Type.Optional(text(256)),
  assignment: Type.Optional(AssignmentSchema),
  truncated: Type.Optional(Type.Boolean()),
  elapsed: Type.Optional(text(64)),
  tokens: Type.Optional(count),
  /** Share of the displayed budget, with capacity as the default basis. */
  context: Type.Optional(amount),
  contextBasis: Type.Optional(Type.Literal('compact')),
  /** Pi's reported cost total for the worker's session so far, in dollars. */
  cost: Type.Optional(amount),
  /** Set only when that total is provider-billed throughout; absent means an estimate. */
  costBilled: Type.Optional(Type.Literal(true)),
  session: Type.Optional(text(4096)),
  /** Set when the completion was journaled on another branch of this session. */
  origin: Type.Optional(Type.Literal('other-branch')),
});
export type EventDetails = Static<typeof EventDetailsSchema>;

const CompletionSchema = Type.Object(
  {
    key: text(512),
    content: Type.String({maxLength: 64_000}),
    details: EventDetailsSchema,
  },
  {additionalProperties: false},
);
export type Completion = Static<typeof CompletionSchema>;

export const JournalSchema = Type.Union([
  Type.Object(
    {
      version: Type.Literal(RECORD_VERSION),
      owner: Type.String({minLength: 1}),
      action: Type.Literal('add'),
      item: CompletionSchema,
    },
    {additionalProperties: false},
  ),
  Type.Object(
    {
      version: Type.Literal(RECORD_VERSION),
      owner: Type.String({minLength: 1}),
      action: Type.Literal('consumed'),
      key: text(512),
    },
    {additionalProperties: false},
  ),
]);
export type Journal = Static<typeof JournalSchema>;

export type Decoded<T> =
  | {readonly ok: true; readonly value: T}
  | {readonly ok: false; readonly error: string};

/** Validates a persisted value against its schema, naming the first violation. */
export function decode<S extends TSchema>(
  schema: S,
  value: unknown,
): Decoded<Static<S>> {
  if (Check(schema, value)) return {ok: true, value};
  const [first] = Errors(schema, value);
  return {
    ok: false,
    error: first
      ? `${first.instancePath || 'record'} ${first.message}`
      : 'invalid record',
  };
}

/** Restore diagnostics: counted once per restore, reported once, never fatal to the remaining records. */
export class RestoreReport {
  private readonly problems: string[] = [];
  private total = 0;

  add(problem: string): void {
    this.total++;
    if (this.problems.length < 3) this.problems.push(problem);
  }

  get count(): number {
    return this.total;
  }

  /** One line for the user, or undefined when every record decoded. */
  message(what: string): string | undefined {
    if (!this.total) return undefined;
    return `${this.total} ${what} record${this.total === 1 ? ' was' : 's were'} incompatible and not restored (${this.problems.join('; ')}${this.total > this.problems.length ? '; …' : ''})`;
  }
}
