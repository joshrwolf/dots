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
  last_event: null,
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
export function source() {
  const calls = [];
  const releases = [];
  return {
    calls,
    releases,
    releaseFailure: undefined,
    async release(owner) {
      releases.push(owner);
      if (this.releaseFailure) throw new Error(this.releaseFailure);
    },
    async show(cwd, id, signal, history, project) {
      calls.push({cwd, id, signal, project});
      const value = snapshot(id);
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
