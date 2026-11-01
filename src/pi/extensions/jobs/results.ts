import {Type, type Static} from 'typebox';
import {Check} from 'typebox/value';
import {MAX_RETAINED, MAX_TIMEOUT_MS, live, type Job} from './protocol.ts';

const nullableText = (maxLength: number) =>
  Type.Union([Type.String({maxLength}), Type.Null()]);
const count = Type.Integer({minimum: 0});
const closed = {additionalProperties: false} as const;
const exit = {
  code: Type.Union([count, Type.Null()]),
  signal: nullableText(32),
};

/** Bounded snapshots: recorded facts only, never output contents or control handles. */
export const JobSchema = Type.Object(
  {
    version: Type.Literal(1),
    id: Type.String({maxLength: 38, pattern: '^j-[a-f0-9-]{36}$'}),
    command: Type.String({maxLength: 8192}),
    cwd: Type.String({maxLength: 4096}),
    directory: Type.String({maxLength: 4096}),
    timeoutMs: Type.Integer({minimum: 1, maximum: MAX_TIMEOUT_MS}),
    startedAt: count,
    finishedAt: Type.Union([count, Type.Null()]),
    state: Type.Union([
      Type.Literal('accepted'),
      Type.Literal('running'),
      Type.Literal('stopping'),
      Type.Literal('finished'),
      Type.Literal('unknown'),
    ]),
    stopIntent: Type.Union([
      Type.Literal('stop'),
      Type.Literal('owner-shutdown'),
      Type.Null(),
    ]),
    facts: Type.Object(
      {
        root: Type.Union([
          Type.Object({...exit, error: nullableText(512)}, closed),
          Type.Null(),
        ]),
        reason: Type.Union([
          Type.Literal('stop'),
          Type.Literal('timeout'),
          Type.Literal('owner-shutdown'),
          Type.Literal('owner-disconnect'),
          Type.Literal('root-exit'),
          Type.Literal('launch-error'),
          Type.Literal('ipc-error'),
          Type.Null(),
        ]),
        termSent: Type.Boolean(),
        killIntent: Type.Boolean(),
        capture: Type.Object(
          {
            bytesSeen: count,
            prefixBytes: count,
            tailBytes: count,
            pendingBytes: count,
            peakPendingBytes: count,
            droppedBytes: count,
            capped: Type.Boolean(),
            streamsEnded: Type.Boolean(),
            prefixFlushed: Type.Boolean(),
            tailFlushed: Type.Boolean(),
            flushExpired: Type.Boolean(),
            error: nullableText(512),
          },
          closed,
        ),
      },
      closed,
    ),
    supervisorExit: Type.Union([Type.Object(exit, closed), Type.Null()]),
    observationLoss: nullableText(1024),
    artifactsEvicted: Type.Boolean(),
  },
  {additionalProperties: false},
);

// At most sixteen restored records plus sixteen admitted in this runtime.
export const JobsSchema = Type.Object(
  {jobs: Type.Array(JobSchema, {maxItems: MAX_RETAINED * 2})},
  {additionalProperties: false},
);
export type JobSnapshot = Static<typeof JobSchema>;

/** Restored entries are historical facts, never handles for processes or deletion. */
export function restore(value: unknown): Job | undefined {
  if (!Check(JobSchema, value)) return undefined;
  const job = structuredClone(value);
  if (live(job)) {
    job.state = 'unknown';
    job.observationLoss =
      'Previous runtime ended; process ownership cannot be restored';
  }
  return job;
}
