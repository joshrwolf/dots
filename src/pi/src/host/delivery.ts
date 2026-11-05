import type {
  ExtensionAPI,
  ExtensionContext,
} from '@earendil-works/pi-coding-agent';

export type TurnMessage = Parameters<ExtensionAPI['sendMessage']>[0];

/**
 * How often held messages recheck idleness when no event will report it, such
 * as a cancelled branch summary.
 */
export const FALLBACK_POLL_MS = 1_000;

interface Held {
  compose: () => TurnMessage | undefined;
  delivered: (() => void) | undefined;
}

/**
 * Custom messages that ask the agent for a turn, such as a fired wake or a
 * review reply, without overlapping or restarting other work.
 *
 * Pi starts a run for a triggering custom message whenever no run is active,
 * even during compaction or a branch summary, and queues it natively only while
 * a run streams. A natively queued custom message is invisible to
 * `hasPendingMessages()` and survives an SDK or RPC abort. So messages are held
 * here until the session is idle and sent then: several held messages become
 * one run. After a run the user cancelled, messages are appended without
 * starting another until the user prompts again.
 *
 * Idleness is rechecked when a run settles and when compaction or tree
 * navigation ends, on the next macrotask: Pi reports compaction and tree
 * navigation before it clears its busy state. A slow poll covers endings no
 * event reports.
 *
 * The owner binds a context on `session_start` and closes on
 * `session_shutdown`; held messages do not outlive the runtime.
 */
export class TurnMessages {
  private readonly pi: Pick<ExtensionAPI, 'sendMessage'>;
  private readonly pollMs: number;
  private context: ExtensionContext | undefined;
  private held: Held[] = [];
  private cancelled = false;
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    pi: Pick<ExtensionAPI, 'on' | 'sendMessage'>,
    pollMs = FALLBACK_POLL_MS,
  ) {
    this.pi = pi;
    this.pollMs = pollMs;
    pi.on('before_agent_start', event => {
      if (event.messageType === 'user') this.cancelled = false;
    });
    pi.on('agent_settled', event => {
      if (event.aborted) this.cancelled = true;
      this.later(0);
    });
    pi.on('session_compact', () => this.later(0));
    pi.on('session_compact_failed', () => this.later(0));
    pi.on('session_tree', () => this.later(0));
  }

  bind(ctx: ExtensionContext): void {
    this.close();
    this.context = ctx;
    this.cancelled = false;
  }

  /**
   * Deliver the message `compose` builds once the session is idle. It is built
   * at delivery, so branch-dependent content reflects the branch it lands on;
   * returning undefined declines a message that became stale while held.
   * The first check follows the current synchronous work, so messages sent
   * together share one run.
   * `delivered` runs once Pi has it, whether or not it starts a run; Pi
   * reports a message appended without a run to no extension event.
   */
  send(compose: () => TurnMessage | undefined, delivered?: () => void): void {
    if (!this.context) return;
    this.held.push({compose, delivered});
    queueMicrotask(() => this.flush());
  }

  /** Whether a message is waiting for the session to become idle. */
  holding(): boolean {
    return this.held.length > 0;
  }

  /** Drop held messages, for example ones a new branch makes stale. */
  discard(): void {
    this.clear();
    this.held = [];
  }

  close(): void {
    this.discard();
    this.context = undefined;
  }

  private clear(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = undefined;
  }

  private later(delay: number): void {
    if (!this.context || !this.held.length) return;
    // An event's recheck replaces a pending fallback poll.
    this.clear();
    this.timer = setTimeout(() => {
      this.timer = undefined;
      this.flush();
    }, delay);
  }

  private flush(): void {
    const ctx = this.context;
    if (!ctx || !this.held.length) return;
    if (!ctx.isIdle()) {
      if (!this.timer) this.later(this.pollMs);
      return;
    }
    this.clear();
    const held = this.held;
    this.held = [];
    const batch = held.flatMap(({compose, delivered}) => {
      const message = compose();
      return message ? [{message, delivered}] : [];
    });
    // While idle, a non-triggering message is appended at once; the last one
    // starts the run that reads them all.
    batch.forEach(({message, delivered}, index) => {
      this.pi.sendMessage(message, {
        triggerTurn: !this.cancelled && index === batch.length - 1,
      });
      delivered?.();
    });
  }
}
