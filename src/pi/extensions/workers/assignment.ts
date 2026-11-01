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

/** Read-only preflight. Claims and acceptance remain the coordinating session's job. */
export async function verifyAssignment(
  value: TaskAssignment,
  cwd: string,
  signal?: AbortSignal,
  client: DotsClient = dots,
): Promise<TaskAssignment> {
  const assignment = decodeAssignment(value);
  let response: unknown;
  try {
    response = await client(
      ['task', 'show', assignment.id, '--project', assignment.project],
      {directory: cwd, cwd, ...(signal ? {signal} : {})},
    );
  } catch (error) {
    throw new Error(
      `Task lookup failed: ${error instanceof Error ? error.message : String(error)}`,
      {cause: error},
    );
  }
  const shown = fields(response);
  if (
    shown?.project !== assignment.project ||
    fields(shown.task)?.id !== assignment.id
  )
    throw new Error('Task lookup returned a different project or task');
  return assignment;
}

export function assignmentContext(
  assignment: TaskAssignment | undefined,
): string {
  return assignment
    ? `Task association: ${JSON.stringify(assignment)}. This is a coordination reference, not execution authority. The parent owns task updates; report outcomes and checks to the parent.`
    : '';
}
