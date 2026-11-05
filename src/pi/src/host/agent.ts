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
 * exported `DOTS_AGENT_OWNER` is only how children learn it: it is always
 * overwritten here, so an owner inherited from a parent process is never
 * adopted. `PI_SESSION_ID` stays the host's.
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
 * The actor for a session that has just started, exported to `env` so
 * native shells, jobs and other children carry it.
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
  env[OWNER_VARIABLE] = agent.owner;
  return agent;
}

/**
 * End `agent` when its session ends without reload. A stale caller cannot
 * end a later runtime's actor.
 */
export function endAgent(
  agent: Agent,
  env: NodeJS.ProcessEnv = process.env,
): void {
  if (holder[BOUND] !== agent) return;
  delete holder[BOUND];
  if (env[OWNER_VARIABLE] === agent.owner) delete env[OWNER_VARIABLE];
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
