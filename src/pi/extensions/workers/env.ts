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
 * parent's Rex identity (`REX_SESSION`, `REX_BLOCK`), which tells an agent it
 * owns a terminal there. `DOTS_REX_SESSION` names the Rex session its work
 * belongs to instead: the parent's own session, or the one the parent
 * inherited, so nested workers keep the top session.
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
    DOTS_REX_SESSION:
      parent.REX_SESSION || parent.DOTS_REX_SESSION || undefined,
    REX_SESSION: undefined,
    REX_BLOCK: undefined,
    PI_SESSION_ID: undefined,
    DOTS_AGENT_OWNER: undefined,
  };
}
