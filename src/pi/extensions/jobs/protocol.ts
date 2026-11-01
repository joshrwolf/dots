import type {JobSnapshot} from './results.ts';

// Wire facts, not inferred success. No process IDs are persisted or used for control.
// The record shape comes from results.ts's schema; this module stays free of
// runtime dependencies because the Node-run supervisor imports it.
export const PREFIX_BYTES = 8 * 1024 * 1024;
export const TAIL_BYTES = 64 * 1024;
export const PENDING_BYTES = 256 * 1024;
export const CHUNK_BYTES = 64 * 1024;
export const GRACE_MS = 3000;
export const DEFAULT_TIMEOUT_MS = 30 * 60 * 1000;
export const MAX_TIMEOUT_MS = 24 * 60 * 60 * 1000;
export const MAX_JOBS = 4;
export const MAX_RETAINED = 16;
export const ENTRY_TYPE = 'dots-job-state';
export const RECORD_TYPE = 'dots-job-result';
export const WAKE_TYPE = 'dots-job-wake';

/** A job record exactly as its schema validates and persists it. */
export type Job = JobSnapshot;
export type SupervisorFacts = Job['facts'];
export type CaptureFacts = SupervisorFacts['capture'];
export type RootOutcome = NonNullable<SupervisorFacts['root']>;
export type StopReason = NonNullable<SupervisorFacts['reason']>;
export type SupervisorMessage =
  {type: 'ready'} | {type: 'facts'; facts: SupervisorFacts};
export type OwnerMessage =
  | {
      type: 'start';
      command: string;
      cwd: string;
      directory: string;
      timeoutMs: number;
    }
  | {type: 'stop'; reason: 'stop' | 'owner-shutdown'};
export function emptyCapture(): CaptureFacts {
  return {
    bytesSeen: 0,
    prefixBytes: 0,
    tailBytes: 0,
    pendingBytes: 0,
    peakPendingBytes: 0,
    droppedBytes: 0,
    capped: false,
    streamsEnded: false,
    prefixFlushed: false,
    tailFlushed: false,
    flushExpired: false,
    error: null,
  };
}
export function emptyFacts(): SupervisorFacts {
  return {
    root: null,
    reason: null,
    termSent: false,
    killIntent: false,
    capture: emptyCapture(),
  };
}
export function live(job: Job): boolean {
  return (
    job.state === 'accepted' ||
    job.state === 'running' ||
    job.state === 'stopping'
  );
}
export function snapshot(job: Job): Job {
  return structuredClone(job);
}

export function summary(job: Job): string {
  const root = job.facts.root;
  const outcome = root
    ? (root.error ??
      (root.signal ? `signal ${root.signal}` : `exit ${root.code}`))
    : 'root outcome unobserved';
  return (
    `${job.id}: ${job.state}; ${outcome}; stop intent ${job.stopIntent ?? 'none'}; cleanup reason ${job.facts.reason ?? 'none'}\n` +
    `Command: ${job.command.slice(0, 256)}${job.command.length > 256 ? '… (command preview limited)' : ''}\n` +
    `Cleanup evidence: TERM ${job.facts.termSent ? 'sent' : 'unobserved'}, group KILL ${job.facts.killIntent ? 'intent recorded' : 'unobserved'}, supervisor ${job.supervisorExit?.signal ?? job.supervisorExit?.code ?? 'not observed exiting'}. No universal containment guarantee.\n` +
    `Capture: ${job.facts.capture.bytesSeen} bytes seen, ${job.facts.capture.prefixBytes} prefix bytes written; capped=${job.facts.capture.capped}, dropped=${job.facts.capture.droppedBytes}, streamsEnded=${job.facts.capture.streamsEnded}, prefixFlushed=${job.facts.capture.prefixFlushed}, tailFlushed=${job.facts.capture.tailFlushed}, flushExpired=${job.facts.capture.flushExpired}${job.facts.capture.error ? `; ${job.facts.capture.error}` : ''}.\n` +
    (job.observationLoss ? `Observation lost: ${job.observationLoss}\n` : '') +
    (job.artifactsEvicted
      ? 'Output artifacts evicted.'
      : `Output prefix: ${job.directory}/output.log\nRecent tail: ${job.directory}/tail.log`)
  );
}
