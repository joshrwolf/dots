import { Type, type Static } from "typebox";
import { MAX_RETAINED, MAX_TIMEOUT_MS, type Job } from "./protocol.ts";

const nullableText = (maxLength: number) => Type.Union([Type.String({ maxLength }), Type.Null()]);
const count = Type.Number({ minimum: 0 });
const exit = {
  code: Type.Union([count, Type.Null()]),
  signal: nullableText(32),
};

/** Bounded snapshots: recorded facts only, never output contents or control handles. */
export const JobSchema = Type.Object({
  version: Type.Literal(1),
  id: Type.String({ maxLength: 38, pattern: "^j-[a-f0-9-]{36}$" }),
  command: Type.String({ maxLength: 8192 }),
  cwd: Type.String({ maxLength: 4096 }),
  directory: Type.String({ maxLength: 4096 }),
  timeoutMs: Type.Number({ minimum: 0, maximum: MAX_TIMEOUT_MS }),
  startedAt: count,
  finishedAt: Type.Union([count, Type.Null()]),
  state: Type.Union([Type.Literal("accepted"), Type.Literal("running"), Type.Literal("stopping"), Type.Literal("finished"), Type.Literal("unknown")]),
  stopIntent: Type.Union([Type.Literal("stop"), Type.Literal("owner-shutdown"), Type.Null()]),
  facts: Type.Object({
    root: Type.Union([Type.Object({ ...exit, error: nullableText(512) }), Type.Null()]),
    reason: Type.Union([Type.Literal("stop"), Type.Literal("timeout"), Type.Literal("owner-shutdown"), Type.Literal("owner-disconnect"), Type.Literal("root-exit"), Type.Literal("launch-error"), Type.Literal("ipc-error"), Type.Null()]),
    termSent: Type.Boolean(),
    killIntent: Type.Boolean(),
    capture: Type.Object({
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
    }),
  }),
  supervisorExit: Type.Union([Type.Object(exit), Type.Null()]),
  observationLoss: nullableText(1024),
  artifactsEvicted: Type.Boolean(),
}, { additionalProperties: false });

// At most sixteen restored records plus sixteen admitted in this runtime.
export const JobsSchema = Type.Object({ jobs: Type.Array(JobSchema, { maxItems: MAX_RETAINED * 2 }) }, { additionalProperties: false });
export type JobSnapshot = Static<typeof JobSchema>;

/** Runtime accessors already return detached snapshots; keep the JSON/schema type explicit. */
export function jobOutput(job: Job): JobSnapshot { return job; }
