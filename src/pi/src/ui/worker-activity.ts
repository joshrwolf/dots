import type {TaskAssignment} from '../../extensions/workers/assignment.ts';

/** Session-local worker observations; absence of a matching assignment is not activity. */
export const WORKER_ACTIVITY_EVENT = 'dots:worker-activity';
export const WORKER_FRAME_MS = 120;
const FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
export const workerFrame = (frame: number): string =>
  FRAMES[frame % FRAMES.length]!;

export interface WorkerActivity {
  readonly assignment: TaskAssignment;
  readonly state: 'running' | 'asking' | 'quiet';
  readonly label: string;
  /** Its current phase, headline or tool call, as the worker widget shows it. */
  readonly doing?: string | undefined;
}
export interface WorkerActivitySnapshot {
  readonly workers: readonly WorkerActivity[];
  readonly frame: number;
}

interface WorkerRow {
  readonly assignment?: TaskAssignment;
  readonly label: string;
  readonly state: string;
  readonly detail?: string | undefined;
  readonly headline?: string | undefined;
  readonly phase?: string | undefined;
  readonly idleMs: number;
  readonly failed?: string | undefined;
  readonly finished?: boolean | undefined;
}

/** Project the same live rows the worker widget uses, without reading a registry in goal UI. */
export function projectWorkerActivity(
  rows: readonly WorkerRow[],
  frame: number,
  quietAfterMs: number,
): WorkerActivitySnapshot {
  return {
    frame,
    workers: rows.flatMap(row => {
      if (
        !row.assignment ||
        row.finished ||
        (row.state !== 'running' && row.state !== 'asking')
      )
        return [];
      const doing = row.phase ?? row.headline ?? row.detail;
      return [
        {
          assignment: row.assignment,
          label: row.label,
          ...(doing ? {doing} : {}),
          state:
            row.state === 'asking'
              ? ('asking' as const)
              : row.failed || row.idleMs >= quietAfterMs
                ? ('quiet' as const)
                : ('running' as const),
        },
      ];
    }),
  };
}

/** Treat other extensions' event payloads as external values, even on a local bus. */
export function workerActivitySnapshot(
  data: unknown,
): WorkerActivitySnapshot | undefined {
  if (!data || typeof data !== 'object') return undefined;
  const value = data as Partial<WorkerActivitySnapshot>;
  if (!Number.isSafeInteger(value.frame) || !Array.isArray(value.workers))
    return undefined;
  if (
    !value.workers.every(
      worker =>
        worker &&
        (worker.state === 'running' ||
          worker.state === 'asking' ||
          worker.state === 'quiet') &&
        typeof worker.label === 'string' &&
        (worker.doing === undefined || typeof worker.doing === 'string') &&
        typeof worker.assignment?.project === 'string' &&
        typeof worker.assignment.id === 'string',
    )
  )
    return undefined;
  return {workers: value.workers, frame: value.frame!};
}
