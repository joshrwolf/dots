/**
 * Human attention a session is responsible for surfacing, shared on Pi's
 * event bus from the goal extension to the status owner. Only a goal's
 * current coordinator publishes, so read-only followers of the same goal
 * never raise a second alert. A snapshot, not a delta: an empty list clears.
 */

export const ATTENTION_EVENT = 'dots:attention';

export interface AttentionRequest {
  readonly id: string;
  readonly title: string;
  /** The goal's title. */
  readonly goal: string;
  /** False when the session coordinates the goal without following it. */
  readonly followed: boolean;
}

export interface AttentionSignal {
  /** Open requests awaiting a person, oldest first. */
  readonly requests: readonly AttentionRequest[];
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
        typeof (item as {title?: unknown}).title === 'string' &&
        typeof (item as {goal?: unknown}).goal === 'string' &&
        typeof (item as {followed?: unknown}).followed === 'boolean',
    )
  );
}
