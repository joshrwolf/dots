import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { RECORD_TYPE, WAKE_TYPE, summary, type Job } from "./protocol.ts";

export function registerDeliveryHooks(pi: ExtensionAPI, current: () => CompletionDelivery | undefined): void {
  pi.on("before_agent_start", (event) => { if (event.messageType === "user") current()?.userStart(); });
  pi.on("context", (event) => { current()?.observed(event.messages); });
  pi.on("agent_before_settle", (event) => current()?.beforeSettle(event.outcome));
  pi.on("agent_settled", (event) => current()?.settled(event.aborted));
}

/** Record and wake are deliberately separate operations. No delivery retries. */
export class CompletionDelivery {
  private readonly pi: ExtensionAPI;
  private readonly context: () => ExtensionContext | undefined;
  private readonly owed = new Set<string>();
  private suppressed = false;
  private closed = false;
  private wake: ReturnType<typeof setImmediate> | undefined;
  constructor(pi: ExtensionAPI, context: () => ExtensionContext | undefined) { this.pi = pi; this.context = context; }

  record(job: Job): void {
    const ctx = this.context();
    if (this.closed || !ctx) return;
    this.pi.sendMessage({ customType: RECORD_TYPE, content: summary(job), display: true, details: { id: job.id, state: job.state } }, { triggerTurn: false });
    if (ctx.signal?.aborted) this.suppressed = true;
    if (this.suppressed) return;
    this.owed.add(job.id);
    if (!ctx.isIdle() || this.wake) return;
    this.wake = setImmediate(() => {
      this.wake = undefined;
      const current = this.context();
      if (this.closed || !current || this.suppressed || current.signal?.aborted || !this.owed.size || !current.isIdle()) return;
      this.owed.clear();
      this.pi.sendMessage({ customType: WAKE_TYPE, content: "Background job results are recorded above. Review them as needed.", display: false }, { triggerTurn: true });
    });
  }
  /** Observe the actual next request context, rather than assume turn_start ordering. */
  observed(messages: readonly unknown[]): void {
    for (const message of messages) {
      if (!message || typeof message !== "object") continue;
      const m = message as { role?: unknown; customType?: unknown; details?: { id?: unknown } };
      if (m.role === "custom" && m.customType === RECORD_TYPE && typeof m.details?.id === "string") this.owed.delete(m.details.id);
    }
  }
  beforeSettle(outcome: string): { continue: true } | undefined {
    if (this.closed || this.suppressed || outcome !== "completed" || !this.owed.size || this.context()?.signal?.aborted) return undefined;
    this.owed.clear();
    return { continue: true };
  }
  settled(aborted: boolean): void {
    if (aborted) { this.suppressed = true; this.owed.clear(); clearImmediate(this.wake); this.wake = undefined; }
  }
  userStart(): void { this.suppressed = false; }
  close(): void { this.closed = true; this.owed.clear(); clearImmediate(this.wake); this.wake = undefined; }
}
