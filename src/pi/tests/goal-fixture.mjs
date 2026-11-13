// Hand-authored shared-goal records; no machine captures or real repository data.
export const task = (id, title, extra = {}) => ({
  id,
  title,
  status: 'open',
  parent: null,
  children: 0,
  owner: null,
  result: null,
  result_event: null,
  evidence: [],
  purged_evidence: [],
  references: [],
  branch: null,
  blocked_by: [],
  goal: null,
  holds: [],
  last_event: null,
  foreign: null,
  ...extra,
});
export const snapshot = (id = 't-123abc', title = 'Consistent exports') => ({
  project: '/example/project',
  now: 100,
  overview: {
    goal: {
      root: task(id, title, {children: 1}),
      brief: '/example/brief.md',
      briefContent: {
        text: '# Design\nUse one shared export interface.\n\nWorking agreement: discuss the reference slice first.',
      },
    },
    tasks: [task('t-456def', 'Reference slice', {parent: id})],
    hidden_closed: 0,
    graph: {
      rows: [{task: 't-456def', lines: ['○']}],
      notes: [],
    },
    open_graph: null,
    progress: {
      total: 1,
      open: 1,
      held: 0,
      done: 0,
      dropped: 0,
      ready: 1,
      claimed: 0,
      review: 0,
    },
  },
});
/**
 * A fake goal source. `coordinator` owns the root claim of `t-123abc` while
 * `status` is open; `coordinating` reports it as the store's root claims do.
 */
export function source() {
  const calls = [];
  const releases = [];
  const coordinations = [];
  return {
    calls,
    releases,
    coordinations,
    releaseFailure: undefined,
    coordinator: null,
    status: 'open',
    /** Set to make `coordinating` fail with this message. */
    coordinatingFail: undefined,
    async release(agent) {
      releases.push(agent.owner);
      if (this.releaseFailure) throw new Error(this.releaseFailure);
    },
    async coordinating(agent) {
      coordinations.push(agent.owner);
      await Promise.resolve();
      if (this.coordinatingFail) throw new Error(this.coordinatingFail);
      // Held and closed roots are not coordination.
      return this.coordinator === agent.owner && this.status === 'open'
        ? [{project: '/example/project', id: 't-123abc'}]
        : [];
    },
    async show(cwd, id, signal, history, project) {
      calls.push({cwd, id, signal, project});
      const value = snapshot(id);
      if (id === 't-123abc') {
        const root = value.overview.goal.root;
        root.status = this.status;
        root.owner = this.status === 'open' ? this.coordinator : null;
      }
      if (this.briefContent)
        value.overview.goal.briefContent = this.briefContent;
      return value;
    },
    async list() {
      return {project: '/example/project', goals: [snapshot().overview.goal]};
    },
    briefContent: undefined,
  };
}
