/**
 * Background work a session owns, shared between extensions on Pi's event bus.
 * Each source publishes the complete list of its live items whenever it
 * changes; an empty list means the source is idle. Snapshots, not deltas, so a
 * missed event can never leave a stale item behind.
 */

export const ACTIVITY_EVENT = 'dots:activity';

export interface ActivityItem {
  /** Record path such as `job/j-1`; see `recordId`. */
  readonly id: string;
  /** Short label, such as the worker's label or the job's command. */
  readonly title: string;
  /** One line about what it is doing now. */
  readonly message?: string;
}

export interface ActivitySnapshot {
  readonly source: string;
  readonly items: readonly ActivityItem[];
}

interface EventBus {
  emit(channel: string, data: unknown): void;
}

export function publishActivity(
  events: EventBus,
  source: string,
  items: readonly ActivityItem[],
): void {
  events.emit(ACTIVITY_EVENT, {source, items} satisfies ActivitySnapshot);
}

const SEGMENT = /[^A-Za-z0-9_.+-]+/g;

/** An OSC 7501 record path, `kind/name`, with each segment cut to the protocol's alphabet and 32 bytes. */
export function recordId(kind: string, name: string): string {
  const segment = (text: string) =>
    text
      .replace(SEGMENT, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 32) || 'x';
  return `${segment(kind)}/${segment(name)}`;
}
