import {randomBytes} from 'node:crypto';

/**
 * The live Pi runtime acting for this process's session: the actor dots
 * records for claims and coordination events.
 *
 * The owner is `pi-<native session id>-<nonce>`. A fresh nonce is minted at
 * every session start except reload, so duplicate processes resuming one
 * session, a restart, `/new`, `/resume` (including back to an earlier
 * session) and `/fork` each get their own actor, while `/reload` and `/tree`
 * keep it. Pi re-imports extension modules on reload; the binding survives
 * as process-global state, which child processes never inherit. The
 * process exports it as `DOTS_AGENT_OWNER` beside the true native
 * `PI_SESSION_ID`, so native bash, `!` commands, jobs and other children
 * agree on both. Binding always overwrites them: values inherited from a
 * parent process are never adopted or left standing.
 *
 * This is cooperative provenance for the task store, not authentication or a
 * fence: any process can set the variable.
 */
export interface Agent {
  /** The native Pi session ID. */
  readonly session: string;
  /** `pi-<session>-<nonce>`. */
  readonly owner: string;
}

/** The variable `dots` reads for the actor; see its `agent::owner`. */
export const OWNER_VARIABLE = 'DOTS_AGENT_OWNER';

export type SessionStartReason =
  'startup' | 'reload' | 'new' | 'resume' | 'fork';

const BOUND = Symbol.for('dots.agent.runtime');
type Holder = {[BOUND]?: Agent};
const holder = globalThis as Holder;

/**
 * The actor for a session that has just started, exported to `env` with its
 * native session ID so native shells, jobs and other children carry both.
 */
export function bindAgent(
  session: string,
  reason: SessionStartReason,
  env: NodeJS.ProcessEnv = process.env,
): Agent {
  if (!session.trim()) throw new Error('Pi reported an empty session ID');
  const bound = holder[BOUND];
  let agent: Agent;
  if (reason === 'reload' && bound) {
    // Reload keeps its session; anything else is a host invariant violation.
    if (bound.session !== session)
      throw new Error(
        `Reload changed the Pi session from ${bound.session} to ${session}; its actor cannot carry over.`,
      );
    agent = bound;
  } else {
    agent = {
      session,
      owner: `pi-${session}-${randomBytes(8).toString('hex')}`,
    };
  }
  holder[BOUND] = agent;
  env.PI_SESSION_ID = agent.session;
  env[OWNER_VARIABLE] = agent.owner;
  return agent;
}

/**
 * The actor bound for this process's current runtime, if any. Capture it
 * synchronously where an operation starts, so work that outlives its runtime
 * never picks up a later one.
 */
export function currentAgent(): Agent | undefined {
  return holder[BOUND];
}

/**
 * End `agent` when its session ends without reload, withdrawing its export.
 * A stale caller cannot end or erase a later runtime's, even one of the same
 * session.
 */
export function endAgent(
  agent: Agent,
  env: NodeJS.ProcessEnv = process.env,
): void {
  if (holder[BOUND] !== agent) return;
  delete holder[BOUND];
  if (env[OWNER_VARIABLE] !== agent.owner) return;
  delete env[OWNER_VARIABLE];
  if (env.PI_SESSION_ID === agent.session) delete env.PI_SESSION_ID;
}

/**
 * The environment for a companion subprocess acting as `agent`. Its identity
 * comes from the caller's captured agent, not the process's current export,
 * so a call that outlives its runtime still acts for that runtime.
 */
export function agentEnv(
  agent: Agent,
  base: NodeJS.ProcessEnv = process.env,
): NodeJS.ProcessEnv {
  return {
    ...base,
    PI_SESSION_ID: agent.session,
    [OWNER_VARIABLE]: agent.owner,
  };
}
