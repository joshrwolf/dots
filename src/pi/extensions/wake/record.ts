import {Type, type Static} from 'typebox';
import {PaneTriggerSchema} from './pane.ts';

export const ENTRY = 'dots.wake';
export const VERSION = 2;
export const MIN_INTERVAL_MS = 60_000;
export const DEFAULT_LIMIT = 50;
export const MAX_LIMIT = 1000;
export const MAX_FUTURE_MS = 30 * 24 * 3_600_000;
/** Default give-up deadline for event triggers. */
export const DEFAULT_FOR_MS = 24 * 3_600_000;

export const OnSchema = Type.Union([
  Type.Literal('always'),
  Type.Literal('change'),
  Type.Literal('failure'),
  Type.Literal('success'),
]);
export type On = Static<typeof OnSchema>;

const CheckSchema = Type.Object(
  {
    command: Type.String({minLength: 1}),
    on: OnSchema,
    lastDigest: Type.Optional(Type.String({maxLength: 256})),
    lastCode: Type.Optional(Type.Integer()),
  },
  {additionalProperties: false},
);
export type Check = Static<typeof CheckSchema>;

export const TimeTriggerSchema = Type.Object(
  {
    kind: Type.Literal('time'),
    /** Next planned fire, epoch ms. */
    at: Type.Integer({minimum: 0}),
    /** Interval in ms for recurring wakes. */
    every: Type.Optional(Type.Integer({minimum: MIN_INTERVAL_MS})),
    check: Type.Optional(CheckSchema),
  },
  {additionalProperties: false},
);
export type TimeTrigger = Static<typeof TimeTriggerSchema>;

/**
 * What a wake waits for. Event triggers (all but `time`) carry their own
 * `deadline`, observer state, and `until`, and share the engine's outcomes,
 * pending-fire merging and restore reconciliation.
 */
export const TriggerSchema = Type.Union([TimeTriggerSchema, PaneTriggerSchema]);
export type Trigger = Static<typeof TriggerSchema>;

/** The persisted record; each change appends the whole record and the latest one wins. */
export const WakeSchema = Type.Object(
  {
    version: Type.Literal(VERSION),
    /** The session id that owns this wake. */
    owner: Type.String({minLength: 1, maxLength: 256}),
    id: Type.String({pattern: '^k-[a-f0-9]{16}$'}),
    text: Type.String(),
    limit: Type.Integer({minimum: 1, maximum: MAX_LIMIT}),
    fired: Type.Integer({minimum: 0}),
    /** False once fired out, timed out, ended or cancelled. */
    active: Type.Boolean(),
    createdAt: Type.Integer({minimum: 0}),
    trigger: TriggerSchema,
  },
  {additionalProperties: false},
);
export type WakeRecord = Static<typeof WakeSchema>;
