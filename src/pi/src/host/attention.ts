/**
 * Human attention a session is responsible for surfacing, shared on Pi's
 * event bus from the goal extension to the status owner. Only a goal's
 * current coordinator publishes, so read-only followers of the same goal
 * never raise a second alert. A snapshot, not a delta: an empty list clears.
 */

export const ATTENTION_EVENT = 'dots:attention';

export interface AttentionSignal {
  /** Open requests awaiting a person, oldest first. */
  readonly requests: readonly {readonly id: string; readonly title: string}[];
}

export function isAttentionSignal(value: unknown): value is AttentionSignal {
  if (!value || typeof value !== 'object') return false;
  const requests = (value as {requests?: unknown}).requests;
  return (
    Array.isArray(requests) &&
    requests.every(
      item =>
        !!item &&
        typeof item === 'object' &&
        typeof (item as {id?: unknown}).id === 'string' &&
        typeof (item as {title?: unknown}).title === 'string',
    )
  );
}
