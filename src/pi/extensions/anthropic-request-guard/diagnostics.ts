import {POLICY, START_TIMEOUT_MS, type Observation} from './guard.ts';

export const ENTRY_TYPE = 'anthropic-request-guard';
export const MAX_ENTRIES = 64;
export const MAX_RECENT = 16;

/** Bounded request facts; no attempt-lineage inference or payload capture. */
export class Diagnostics {
  private counts = {started: 0, timeout: 0, cancelled: 0, ended: 0};
  private totalStartMs = 0;
  private maxStartMs = 0;
  private recent: Observation[] = [];
  readonly hostVersion: string;
  constructor(hostVersion: string) {
    this.hostVersion = hostVersion;
  }

  record(observation: Observation): void {
    // Construct the stored shape, never spread adapter-owned data into a log.
    const item = {
      model: observation.model.slice(0, 160),
      outcome: observation.outcome,
      elapsedMs: observation.elapsedMs,
    };
    this.counts[item.outcome]++;
    if (item.outcome === 'started') {
      this.totalStartMs += item.elapsedMs;
      this.maxStartMs = Math.max(this.maxStartMs, item.elapsedMs);
    }
    this.recent.push(item);
    if (this.recent.length > MAX_RECENT) this.recent.shift();
  }

  snapshot() {
    return {
      policy: POLICY,
      hostVersion: this.hostVersion,
      timeoutMs: START_TIMEOUT_MS,
      counts: {...this.counts},
      meanStartMs: this.counts.started
        ? Math.round(this.totalStartMs / this.counts.started)
        : null,
      maxStartMs: this.counts.started ? this.maxStartMs : null,
      recent: this.recent.map(item => ({...item})),
    };
  }
}
