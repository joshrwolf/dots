/** Who a worker is in its tree, as its spawner assigns it. */
export interface WorkerIdentity {
  id: string;
  depth: number;
  root: string;
}

/**
 * Environment overrides for a worker's Pi; an undefined value removes the
 * variable.
 *
 * A headless worker has no terminal of its own, so it never inherits the
 * parent's `REX_BLOCK`, which tells an agent it owns a terminal there.
 * `DOTS_REX_BLOCK` names the terminal its work belongs to instead: the
 * parent's own, or the one the parent inherited, so nested workers keep the
 * top session's terminal, and `dots` can ask Rex which session holds it.
 *
 * A worker is its own agent. It inherits neither a session ID nor the
 * parent's runtime actor (`DOTS_AGENT_OWNER`); its own companion binds one
 * for its session. `DOTS_WORKER_ID` marks it as a worker, which reports to
 * its parent rather than coordinating goal work.
 */
export function workerEnv(
  worker: WorkerIdentity,
  parent: NodeJS.ProcessEnv,
): Record<string, string | undefined> {
  return {
    DOTS_WORKER_ID: worker.id,
    DOTS_WORKER_DEPTH: String(worker.depth),
    DOTS_WORKER_ROOT: worker.root,
    DOTS_REX_BLOCK: parent.REX_BLOCK || parent.DOTS_REX_BLOCK || undefined,
    REX_BLOCK: undefined,
    PI_SESSION_ID: undefined,
    DOTS_AGENT_OWNER: undefined,
  };
}
