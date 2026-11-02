import type {
  BoundaryState,
  ExtensionAPI,
  ExtensionContext,
} from '@earendil-works/pi-coding-agent';
import {labelText} from '../../src/index.ts';
import {fields} from './protocol.ts';
import {
  JournalSchema,
  OUTBOX_ENTRY,
  RECORD_VERSION,
  RestoreReport,
  decode,
  type Completion,
  type EventDetails,
  type Journal,
} from './record.ts';

export const COMPLETION_HOLD_MS = 100;
const EVENT = 'dots.worker.event';

type Delivery = {
  customType: typeof EVENT;
  content: string;
  display: true;
  details: EventDetails & {deliveryIds: string[]};
};

function delivery(items: readonly Completion[]): Delivery {
  const ids = items.map(item => item.key);
  const [only] = items;
  if (only && items.length === 1)
    return {
      customType: EVENT,
      content: only.content,
      display: true,
      details: {...only.details, deliveryIds: ids},
    };
  // Preserve every identity and status even if dozens of children finish together.
  let space = 12_000;
  const lines = items.map(item => {
    const header = `Worker ${item.details.label ?? item.details.id} [${item.details.id}] ${item.details.kind}${item.details.origin ? ' · from another branch of this session' : ''}; full text: worker_status { id: "${item.details.id}" }`;
    const excerpt = item.content.split('\n\n').slice(1).join('\n\n');
    const preview = space > 0 ? excerpt.slice(0, Math.min(1000, space)) : '';
    space -= preview.length;
    return `${header}${preview ? `\n${preview}` : ''}`;
  });
  return {
    customType: EVENT,
    display: true,
    content: `Workers completed (${items.length})\n\n${lines.join('\n\n---\n\n')}`,
    details: {
      id: 'workers',
      label: `${items.length} workers`,
      kind: 'completed',
      deliveryIds: ids,
    },
  };
}

/** The delivery ids a branch entry or committed message carries, if it is one of our worker events. */
function receipts(value: unknown): readonly string[] {
  const entry = fields(value);
  if (entry?.customType !== EVENT) return [];
  const ids = fields(entry.details)?.deliveryIds;
  return Array.isArray(ids)
    ? ids.filter((id): id is string => typeof id === 'string')
    : [];
}

/** A completion journaled on another branch keeps its origin visible where it is delivered. */
function fromOtherBranch(item: Completion): Completion {
  return {
    key: item.key,
    content: item.content.replace(
      /^[^\n]*/,
      header => `${header} · from another branch of this session`,
    ),
    details: {...item.details, origin: 'other-branch'},
  };
}

/**
 * Journals unconsumed completions as session resources; delivery receipts are
 * branch-local. A completion offered on one branch and received there is
 * offered once more after /tree moves to a branch that has no receipt.
 */
export class CompletionOutbox {
  private readonly pending = new Map<string, Completion>();
  private readonly inFlight = new Set<string>();
  private readonly pi: Pick<ExtensionAPI, 'appendEntry'>;
  private readonly idle: (message: Delivery) => void;
  private readonly canFlushIdle: () => boolean;
  private readonly onError: (error: unknown) => void;
  private owner: string | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private closed = false;

  constructor(
    pi: Pick<ExtensionAPI, 'appendEntry'>,
    idle: (message: Delivery) => void,
    canFlushIdle: () => boolean,
    onError: (error: unknown) => void = error => {
      console.error(
        'worker completion delivery failed; result retained:',
        error,
      );
    },
  ) {
    this.pi = pi;
    this.idle = idle;
    this.canFlushIdle = canFlushIdle;
    this.onError = onError;
  }

  /**
   * Rebuilds pending completions for `owner` from the session's `entries` (file
   * order) less the receipts and consumptions on the current `branch`.
   * Returns a diagnostic when some journal records could not be decoded.
   */
  restore(
    owner: string,
    entries: readonly unknown[],
    branch: readonly unknown[],
  ): string | undefined {
    this.owner = owner;
    this.pending.clear();
    this.inFlight.clear();
    this.closed = false;
    const report = new RestoreReport();
    const onBranch = new Set(branch.map(entry => fields(entry)?.id));
    const journal = (entry: unknown) => {
      const record = fields(entry);
      return record?.type === 'custom' && record.customType === OUTBOX_ENTRY
        ? record
        : undefined;
    };
    // Session-wide: each key's latest add. An unreadable record that still names
    // a result key (item.key), whatever its action says, leaves that key with
    // nothing, never with an older add.
    const added = new Map<string, Completion | undefined>();
    for (const entry of entries) {
      const record = journal(entry);
      if (!record) continue;
      const decoded = decode(JournalSchema, record.data);
      if (!decoded.ok) {
        report.add(decoded.error);
        const key = payloadKey(record.data);
        if (key !== undefined) added.set(key, undefined);
        continue;
      }
      const value = decoded.value;
      if (value.owner !== owner || value.action !== 'add') continue;
      added.set(
        value.item.key,
        onBranch.has(record.id) ? value.item : fromOtherBranch(value.item),
      );
    }
    for (const [key, item] of added) if (item) this.pending.set(key, item);
    // Branch-local: consumptions and delivery receipts. An unreadable record on
    // this branch that names a receipt key (reported above), whatever its action
    // says, withholds that key here rather than risk a second delivery.
    for (const entry of branch) {
      const record = fields(entry);
      if (record?.type === 'custom_message') {
        for (const id of receipts(record)) this.pending.delete(id);
        continue;
      }
      const custom = journal(entry);
      if (!custom) continue;
      const decoded = decode(JournalSchema, custom.data);
      const key = !decoded.ok
        ? receiptKey(custom.data)
        : decoded.value.owner === owner && decoded.value.action === 'consumed'
          ? decoded.value.key
          : undefined;
      if (key !== undefined) this.pending.delete(key);
    }
    this.schedule();
    return report.message('worker completion');
  }

  offer(item: Completion): void {
    if (this.closed) return;
    const snapshot = this.journal(item);
    this.pending.set(snapshot.key, snapshot);
    this.schedule();
  }

  /** A completion a blocked waiter already returned: journaled for the session, consumed on this branch only. */
  taken(item: Completion): void {
    if (this.closed) return;
    this.journal(item);
    this.pi.appendEntry(OUTBOX_ENTRY, {
      version: RECORD_VERSION,
      owner: this.bound(),
      action: 'consumed',
      key: item.key,
    } satisfies Journal);
  }

  private journal(item: Completion): Completion {
    const snapshot: Completion = {
      key: item.key,
      content: item.content,
      details: structuredClone(item.details),
    };
    this.pi.appendEntry(OUTBOX_ENTRY, {
      version: RECORD_VERSION,
      owner: this.bound(),
      action: 'add',
      item: snapshot,
    } satisfies Journal);
    return snapshot;
  }

  /** A tool result already contains this outcome; don't inject it into the model a second time. */
  consume(key: string): void {
    if (!this.pending.has(key) || this.inFlight.has(key)) return;
    this.pi.appendEntry(OUTBOX_ENTRY, {
      version: RECORD_VERSION,
      owner: this.bound(),
      action: 'consumed',
      key,
    } satisfies Journal);
    this.pending.delete(key);
  }

  /** Ordinary custom messages emit message_end; boundary drafts only append entries. */
  committed(message: unknown): void {
    const record = fields(message);
    if (record?.role !== 'custom') return;
    for (const id of receipts(record)) {
      this.pending.delete(id);
      this.inFlight.delete(id);
    }
    this.schedule();
  }

  /** After settlement, reconcile boundary drafts against the actual branch. */
  reconcile(branch: readonly unknown[]): void {
    for (
      let index = branch.length - 1;
      index >= 0 && this.pending.size > 0;
      index--
    ) {
      const entry = fields(branch[index]);
      if (entry?.type !== 'custom_message') continue;
      for (const id of receipts(entry)) {
        this.pending.delete(id);
        this.inFlight.delete(id);
      }
    }
  }

  hasPending(): boolean {
    return this.pending.size > 0;
  }

  /** Only a completion not yet drafted can be consumed by a late waiter. */
  queued(key: string): Completion | undefined {
    return this.inFlight.has(key) ? undefined : this.pending.get(key);
  }

  async hold(): Promise<void> {
    if (!this.closed)
      await new Promise<void>(resolve =>
        setTimeout(resolve, COMPLETION_HOLD_MS),
      );
  }

  /** At a supported turn/settle boundary, give Pi a custom-message draft. */
  draft(): (Delivery & {type: 'custom_message'}) | undefined {
    const items = [...this.pending.values()].filter(
      item => !this.inFlight.has(item.key),
    );
    if (this.closed || items.length === 0) return undefined;
    for (const item of items) this.inFlight.add(item.key);
    return {type: 'custom_message', ...delivery(items)};
  }

  settled(): void {
    // An invalid boundary commits no message; make those items eligible again.
    this.inFlight.clear();
    this.schedule();
  }

  schedule(): void {
    if (this.closed || !this.hasPending() || this.timer) return;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      try {
        if (this.closed || !this.canFlushIdle()) return;
        const draft = this.draft();
        if (draft)
          this.idle({
            customType: draft.customType,
            content: draft.content,
            display: true,
            details: draft.details,
          });
      } catch (error) {
        // Retain the journaled result for the next boundary or reload. Do not
        // throw from a timer or immediately reschedule a failing delivery.
        this.inFlight.clear();
        try {
          this.onError(error);
        } catch (reportError) {
          console.error(
            'worker completion delivery error could not be reported:',
            reportError,
          );
        }
      }
    }, COMPLETION_HOLD_MS);
  }

  close(): void {
    this.closed = true;
    if (this.timer) clearTimeout(this.timer);
    this.timer = undefined;
    this.inFlight.clear();
  }

  private bound(): string {
    if (this.owner === undefined)
      throw new Error('worker completions have no owning session yet');
    return this.owner;
  }
}

/**
 * The result key an undecodable journal record still names. Its action is not
 * trusted: a damaged discriminant must not let an older record stand.
 */
function payloadKey(data: unknown): string | undefined {
  const key = fields(fields(data)?.item)?.key;
  return typeof key === 'string' ? key : undefined;
}

/** The receipt key an undecodable journal record still names, whatever its action. */
function receiptKey(data: unknown): string | undefined {
  const key = fields(data)?.key;
  return typeof key === 'string' ? key : undefined;
}

/** Pi's turn and settle boundaries own continuations; an idle hold coalesces nearby results. */
export class WorkerDelivery {
  readonly outbox: CompletionOutbox;
  private readonly live: () => boolean;
  private readonly active: () => boolean;
  private running = false;
  private interrupted = false;
  private closed = false;
  private context: ExtensionContext | undefined;

  constructor(pi: ExtensionAPI, live: () => boolean, active: () => boolean) {
    this.live = live;
    this.active = active;
    this.outbox = new CompletionOutbox(
      pi,
      message => {
        pi.sendMessage(message, {
          triggerTurn: !this.interrupted,
          deliverAs: 'steer',
        });
        // Non-triggering idle delivery appends immediately; unlike a boundary
        // draft, it has no subsequent settlement in which to reconcile receipts.
        if (this.interrupted && this.context)
          this.outbox.reconcile(this.context.sessionManager.getBranch());
      },
      () => !this.closed && this.active() && !this.running,
      error => {
        this.warn(
          `Worker completion delivery failed; result retained for the next turn or reload: ${labelText(String(error)).slice(0, 500)}`,
        );
      },
    );
    pi.on('session_start', (_event, ctx) => {
      if (!this.closed) this.restore(ctx);
    });
    // Resources are session-scoped; only receipts follow the branch.
    pi.on('session_tree', (_event, ctx) => {
      if (!this.closed) this.restore(ctx);
    });
    pi.on('before_agent_start', (_event, ctx) => {
      if (this.closed) return;
      this.context = ctx;
      this.running = true;
      this.interrupted = false;
    });
    pi.on('agent_start', () => {
      if (!this.closed) this.running = true;
    });
    pi.on('message_end', event => {
      if (!this.closed) this.outbox.committed(event.message);
    });
    pi.on('turn_end', event =>
      this.closed ? undefined : this.boundary(event, this.outbox.hasPending()),
    );
    pi.on('agent_before_settle', event =>
      this.closed ? undefined : this.boundary(event, true),
    );
    pi.on('agent_settled', (event, ctx) => {
      if (this.closed) return;
      this.context = ctx;
      this.running = false;
      if (event.aborted) this.interrupted = true;
      this.outbox.reconcile(ctx.sessionManager.getBranch());
      this.outbox.settled();
    });
  }

  canWake(): boolean {
    return !this.interrupted;
  }

  private restore(ctx: ExtensionContext): void {
    this.context = ctx;
    const problem = this.outbox.restore(
      ctx.sessionManager.getSessionId(),
      ctx.sessionManager.getEntries(),
      ctx.sessionManager.getBranch(),
    );
    if (problem) this.warn(problem);
  }

  private warn(message: string): void {
    if (this.context?.hasUI) this.context.ui.notify(message, 'warning');
    else console.error(message);
  }

  private async boundary(event: BoundaryState, hold: boolean) {
    this.interrupted = event.outcome !== 'completed';
    if (
      hold &&
      event.outcome === 'completed' &&
      !this.closed &&
      (this.outbox.hasPending() || this.live())
    )
      await this.outbox.hold();
    if (this.closed) return undefined;
    const draft = this.outbox.draft();
    if (!draft) return undefined;
    // The new model-visible custom message can make a previously final
    // context continuable. Errors and aborts still never start another turn.
    const canContinue =
      event.context.canContinue || draft.type === 'custom_message';
    return {
      entries: [...event.entries, draft],
      continue:
        event.continue || (event.outcome === 'completed' && canContinue),
    };
  }

  close(): void {
    this.closed = true;
    this.context = undefined;
    this.outbox.close();
  }
}
