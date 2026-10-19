// Wire facts, not inferred success. No process IDs are persisted or used for control.
export const PREFIX_BYTES = 8 * 1024 * 1024;
export const TAIL_BYTES = 64 * 1024;
export const PENDING_BYTES = 256 * 1024;
export const CHUNK_BYTES = 64 * 1024;
export const GRACE_MS = 3000;
export const DEFAULT_TIMEOUT_MS = 30 * 60 * 1000;
export const MAX_TIMEOUT_MS = 24 * 60 * 60 * 1000;
export const MAX_JOBS = 4;
export const MAX_RETAINED = 16;
export const ENTRY_TYPE = "dots-job-state";
export const RECORD_TYPE = "dots-job-result";
export const WAKE_TYPE = "dots-job-wake";

export type StopReason = "stop" | "timeout" | "owner-shutdown" | "owner-disconnect" | "root-exit" | "launch-error" | "ipc-error";
export interface RootOutcome { code: number | null; signal: string | null; error: string | null }
export interface CaptureFacts {
  bytesSeen: number;
  prefixBytes: number;
  tailBytes: number;
  pendingBytes: number;
  peakPendingBytes: number;
  droppedBytes: number;
  capped: boolean;
  streamsEnded: boolean;
  prefixFlushed: boolean;
  tailFlushed: boolean;
  flushExpired: boolean;
  error: string | null;
}
export interface SupervisorFacts {
  root: RootOutcome | null;
  reason: StopReason | null;
  termSent: boolean;
  killIntent: boolean;
  capture: CaptureFacts;
}
export type SupervisorMessage = { type: "ready" } | { type: "facts"; facts: SupervisorFacts };
export type OwnerMessage = { type: "start"; command: string; cwd: string; directory: string; timeoutMs: number } | { type: "stop"; reason: "stop" | "owner-shutdown" };
export interface Job {
  version: 1;
  id: string;
  command: string;
  cwd: string;
  directory: string;
  timeoutMs: number;
  startedAt: number;
  finishedAt: number | null;
  state: "accepted" | "running" | "stopping" | "finished" | "unknown";
  stopIntent: "stop" | "owner-shutdown" | null;
  facts: SupervisorFacts;
  supervisorExit: { code: number | null; signal: string | null } | null;
  observationLoss: string | null;
  artifactsEvicted: boolean;
}
export function emptyCapture(): CaptureFacts {
  return { bytesSeen: 0, prefixBytes: 0, tailBytes: 0, pendingBytes: 0, peakPendingBytes: 0, droppedBytes: 0, capped: false, streamsEnded: false, prefixFlushed: false, tailFlushed: false, flushExpired: false, error: null };
}
export function emptyFacts(): SupervisorFacts { return { root: null, reason: null, termSent: false, killIntent: false, capture: emptyCapture() }; }
export function live(job: Job): boolean { return job.state === "accepted" || job.state === "running" || job.state === "stopping"; }
export function snapshot(job: Job): Job { return structuredClone(job); }

/** Restored entries are historical facts, never handles for processes or deletion. */
export function restore(value: unknown): Job | undefined {
  if (!value || typeof value !== "object") return undefined;
  const j = value as Partial<Job>;
  const str = (v: unknown, max: number): v is string => typeof v === "string" && v.length <= max;
  const number = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v) && v >= 0;
  const nullableString = (v: unknown, max: number): boolean => v === null || str(v, max);
  const outcome = (v: unknown): boolean => {
    if (v === null) return true;
    if (!v || typeof v !== "object") return false;
    const o = v as Partial<RootOutcome>;
    return (o.code === null || number(o.code)) && nullableString(o.signal, 32);
  };
  const f = j.facts;
  const c = f?.capture;
  if (j.version !== 1 || !str(j.id, 38) || !/^j-[a-f0-9-]{36}$/.test(j.id) ||
      !str(j.command, 8192) || !str(j.cwd, 4096) || !str(j.directory, 4096) || !number(j.startedAt) ||
      !number(j.timeoutMs) || j.timeoutMs > MAX_TIMEOUT_MS || !(j.finishedAt === null || number(j.finishedAt)) ||
      !["accepted", "running", "stopping", "finished", "unknown"].includes(j.state ?? "") ||
      ![null, "stop", "owner-shutdown"].includes(j.stopIntent as Job["stopIntent"]) ||
      !nullableString(j.observationLoss, 1024) || typeof j.artifactsEvicted !== "boolean" || !outcome(j.supervisorExit) ||
      !f || !outcome(f.root) || (f.root !== null && !nullableString(f.root.error, 512)) ||
      ![null, "stop", "timeout", "owner-shutdown", "owner-disconnect", "root-exit", "launch-error", "ipc-error"].includes(f.reason) ||
      typeof f.termSent !== "boolean" || typeof f.killIntent !== "boolean" || !c ||
      ![c.bytesSeen, c.prefixBytes, c.tailBytes, c.pendingBytes, c.peakPendingBytes, c.droppedBytes].every(number) ||
      ![c.capped, c.streamsEnded, c.prefixFlushed, c.tailFlushed, c.flushExpired].every((v) => typeof v === "boolean") ||
      !nullableString(c.error, 512)) return undefined;
  // Reconstruct only our bounded, versioned fields; runtime handles remain empty.
  const result: Job = {
    version: 1, id: j.id, command: j.command, cwd: j.cwd, directory: j.directory,
    startedAt: j.startedAt, timeoutMs: j.timeoutMs, finishedAt: j.finishedAt as Job["finishedAt"],
    state: j.state as Job["state"], stopIntent: j.stopIntent as Job["stopIntent"],
    observationLoss: j.observationLoss as Job["observationLoss"], artifactsEvicted: j.artifactsEvicted,
    supervisorExit: j.supervisorExit === null ? null : { code: j.supervisorExit!.code, signal: j.supervisorExit!.signal },
    facts: { root: f.root === null ? null : { code: f.root.code, signal: f.root.signal, error: f.root.error },
      reason: f.reason, termSent: f.termSent, killIntent: f.killIntent,
      capture: { bytesSeen: c.bytesSeen, prefixBytes: c.prefixBytes, tailBytes: c.tailBytes, pendingBytes: c.pendingBytes,
        peakPendingBytes: c.peakPendingBytes, droppedBytes: c.droppedBytes, capped: c.capped, streamsEnded: c.streamsEnded,
        prefixFlushed: c.prefixFlushed, tailFlushed: c.tailFlushed, flushExpired: c.flushExpired, error: c.error } },
  };
  if (live(result)) { result.state = "unknown"; result.observationLoss = "Previous runtime ended; process ownership cannot be restored"; }
  return result;
}

export function summary(job: Job): string {
  const root = job.facts.root;
  const outcome = root ? root.error ?? (root.signal ? `signal ${root.signal}` : `exit ${root.code}`) : "root outcome unobserved";
  return `${job.id}: ${job.state}; ${outcome}; stop intent ${job.stopIntent ?? "none"}; cleanup reason ${job.facts.reason ?? "none"}\n` +
    `Command: ${job.command.slice(0, 256)}${job.command.length > 256 ? "… (command preview limited)" : ""}\n` +
    `Cleanup evidence: TERM ${job.facts.termSent ? "sent" : "unobserved"}, group KILL ${job.facts.killIntent ? "intent recorded" : "unobserved"}, supervisor ${job.supervisorExit?.signal ?? job.supervisorExit?.code ?? "not observed exiting"}. No universal containment guarantee.\n` +
    `Capture: ${job.facts.capture.bytesSeen} bytes seen, ${job.facts.capture.prefixBytes} prefix bytes written; capped=${job.facts.capture.capped}, dropped=${job.facts.capture.droppedBytes}, streamsEnded=${job.facts.capture.streamsEnded}, prefixFlushed=${job.facts.capture.prefixFlushed}, tailFlushed=${job.facts.capture.tailFlushed}, flushExpired=${job.facts.capture.flushExpired}${job.facts.capture.error ? `; ${job.facts.capture.error}` : ""}.\n` +
    (job.observationLoss ? `Observation lost: ${job.observationLoss}\n` : "") +
    (job.artifactsEvicted ? "Output artifacts evicted." : `Output prefix: ${job.directory}/output.log\nRecent tail: ${job.directory}/tail.log`);
}
