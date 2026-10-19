// Hand-authored shared-goal records; no machine captures or real repository data.
export const task = (id, title, extra = {}) => ({ id, title, status: "open", parent: null, children: 0, owner: null, lease_until: null, pr: null, branch: null, blocked_by: [], ...extra });
export const snapshot = (id = "t-123abc", title = "Consistent exports") => ({ project: "/example/project", now: 100,
  overview: { goal: { root: task(id, title, { children: 1 }), brief: "/example/brief.md" },
    tasks: [task("t-456def", "Reference slice", { parent: id })],
    hidden_closed: 0,
    progress: { total: 1, open: 1, held: 0, done: 0, dropped: 0, ready: 1, claimed: 0, with_pr: 0 } },
});
export function source() {
  const calls = [];
  return { calls,
    async show(cwd, id, signal) { calls.push({ cwd, id, signal }); return snapshot(id); },
    async list() { return { project: "/example/project", goals: [snapshot().overview.goal] }; },
    async brief() { return { text: "# Design\nUse one shared export interface.\n\nWorking agreement: discuss the reference slice first." }; },
  };
}
