import {dots, type DotsOptions} from '@joshrwolf/pi-kit/dots';
import {AssignmentSchema, decode} from './record.ts';
import {fields} from './protocol.ts';
import type {Static} from 'typebox';

/** Explicit task identity, independent of a worker's checkout or session owner. */
export type TaskAssignment = Static<typeof AssignmentSchema>;

/** The `dots` JSON client: a test seam, defaulting to the shared pi-kit runner. */
export type DotsClient = (
  args: readonly string[],
  options: DotsOptions,
) => Promise<unknown>;

export function decodeAssignment(value: unknown): TaskAssignment {
  const decoded = decode(AssignmentSchema, value);
  if (!decoded.ok)
    throw new Error(
      `Task assignment needs the project key returned by dots task and an id of t- followed by hexadecimal digits (${decoded.error})`,
    );
  return decoded.value;
}

/** The fields of a fresh `dots task show` this preflight relies on. */
interface ShownTask {
  readonly status: unknown;
  readonly owner: unknown;
  /** The goal root the task belongs to (itself for a root), or null. */
  readonly goal: string | null;
  /** Held ancestors that hold the task. */
  readonly holds: readonly unknown[];
}

/**
 * Read-only, cooperative preflight. Claims and acceptance remain the
 * coordinating session's job. Assigning a goal's task is dispatching goal
 * work, so it also requires `actor`, captured by the caller when the spawn
 * started, to be the goal's coordinator (its root claim's owner), and neither
 * the task nor the goal root to be held or below a held ancestor. Work
 * outside goals keeps the plain identity check.
 * This reads current records once and is not atomic with the spawn: the
 * store's own coordinator checks still govern every mutation, and nothing
 * here fences a takeover after it.
 */
export async function verifyAssignment(
  value: TaskAssignment,
  cwd: string,
  actor: string | undefined,
  signal?: AbortSignal,
  client: DotsClient = dots,
): Promise<TaskAssignment> {
  const assignment = decodeAssignment(value);
  const show = async (id: string): Promise<ShownTask> => {
    let response: unknown;
    try {
      response = await client(
        ['task', 'show', id, '--project', assignment.project],
        {directory: cwd, cwd, ...(signal ? {signal} : {})},
      );
    } catch (error) {
      throw new Error(
        `Task lookup failed: ${error instanceof Error ? error.message : String(error)}`,
        {cause: error},
      );
    }
    const shown = fields(response);
    const task = fields(shown?.task);
    if (shown?.project !== assignment.project || task?.id !== id)
      throw new Error('Task lookup returned a different project or task');
    const goal = task.goal;
    if (
      !(goal === null || (typeof goal === 'string' && TASK_ID.test(goal))) ||
      !Array.isArray(task.holds)
    )
      throw new Error(
        'Task lookup lacks goal association or holds; update the installed dots',
      );
    return {status: task.status, owner: task.owner, goal, holds: task.holds};
  };
  const task = await show(assignment.id);
  if (task.goal === null) return assignment;
  const goal = task.goal;
  const root = goal === assignment.id ? task : await show(goal);
  if (root.goal !== goal)
    throw new Error(`Task lookup returned ${goal} outside its own goal`);
  if (actor === undefined || root.owner !== actor)
    throw new Error(
      `${assignment.id} belongs to goal ${goal}, ${typeof root.owner === 'string' ? `coordinated by ${root.owner}` : 'which has no coordinator'}; only its coordinator dispatches goal work (this runtime: ${actor ?? 'unbound'})`,
    );
  // The root read is the fresher one, so its goal-wide blockers count too.
  for (const [id, shown] of [
    [assignment.id, task],
    [goal, root],
  ] as const) {
    if (shown.status === 'held')
      throw new Error(`${id} is held; resume it before dispatch`);
    if (shown.holds.length)
      throw new Error(
        `${id} is held by ${shown.holds.map(describeHold).join('; ')}; resolve that before dispatch`,
      );
  }
  return assignment;
}

const TASK_ID = /^t-[a-f0-9]+$/;

function describeHold(value: unknown): string {
  const hold = fields(value);
  if (hold?.kind === 'held_ancestor' && typeof hold.task === 'string')
    return `held ancestor ${hold.task}`;
  return 'an unrecognized hold';
}

export function assignmentContext(
  assignment: TaskAssignment | undefined,
): string {
  return assignment
    ? `Task association: ${JSON.stringify(assignment)}. This is a coordination reference, not execution authority. The parent owns task updates; report outcomes and checks to the parent.`
    : '';
}
