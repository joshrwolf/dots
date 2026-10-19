import type { GoalSnapshot, GoalTask } from "./source.ts";

export interface LeafTotals { total: number; done: number; held: number; dropped: number; claimed: number; waiting: number }
export interface GoalNode {
  readonly task: GoalTask;
  readonly children: GoalNode[];
  parent: GoalNode | undefined;
  totals: LeafTotals | undefined;
  rank: number;
}
export interface GoalTree {
  readonly roots: readonly GoalNode[];
  readonly nodes: ReadonlyMap<string, GoalNode>;
  readonly visible: ReadonlySet<string>;
  readonly warning: string | undefined;
}
export interface TreeRow { readonly node: GoalNode; readonly depth: number; readonly expanded: boolean }
export interface TreeState { readonly collapsed: ReadonlySet<string>; readonly selected: string | undefined }

export function taskRank(task: GoalTask, now: number): number {
  if (task.status === "held") return 1;
  if (task.status === "done") return 4;
  if (task.status === "dropped") return 5;
  if (task.children > 0) return 6;
  if (task.owner && task.lease_until !== null && task.lease_until > now) return 0;
  return task.blocked_by.length ? 2 : 3;
}

/** Iterative traversal: malformed parents/cycles cannot recurse or invent totals. */
export function goalTree(snapshot: GoalSnapshot): GoalTree {
  const { overview, now } = snapshot;
  const records = snapshot.allTasks ?? overview.tasks;
  const nodes = new Map<string, GoalNode>();
  let invalid = false;
  for (const task of records) {
    if (nodes.has(task.id) || task.id === overview.goal.root.id) { invalid = true; continue; }
    nodes.set(task.id, { task, children: [], parent: undefined, totals: undefined, rank: taskRank(task, now) });
  }
  const roots: GoalNode[] = [];
  for (const node of nodes.values()) {
    if (node.task.parent === overview.goal.root.id) roots.push(node);
    else {
      const parent = node.task.parent ? nodes.get(node.task.parent) : undefined;
      if (parent && parent !== node) { node.parent = parent; parent.children.push(node); }
      else { invalid = true; roots.push(node); }
    }
  }
  const visited = new Set<string>();
  const order: GoalNode[] = [];
  const visit = (root: GoalNode) => {
    const stack = [root];
    while (stack.length) {
      const node = stack.pop()!;
      if (visited.has(node.task.id)) { invalid = true; continue; }
      visited.add(node.task.id); order.push(node);
      for (let i = node.children.length - 1; i >= 0; i--) stack.push(node.children[i]!);
    }
  };
  for (const root of roots) visit(root);
  for (const node of nodes.values()) if (!visited.has(node.task.id)) { invalid = true; roots.push(node); visit(node); }
  const leaves = order.filter((node) => node.task.children === 0);
  const complete = !invalid && (snapshot.allTasks !== undefined || overview.hidden_closed === 0)
    && roots.length === overview.goal.root.children
    && order.every((node) => node.children.length === node.task.children)
    && leaves.length === overview.progress.total
    && leaves.filter((node) => node.task.status === "open" && node.task.owner && node.task.lease_until !== null && node.task.lease_until > now).length === overview.progress.claimed
    && (["open", "done", "held", "dropped"] as const).every((status) => leaves.filter((node) => node.task.status === status).length === overview.progress[status]);
  for (const node of order.reverse()) {
    if (node.task.children > 0 || node.children.length) {
      node.rank = Math.min(node.rank, ...node.children.map((child) => child.rank));
      if (complete) {
        const totals: LeafTotals = { total: 0, done: 0, held: 0, dropped: 0, claimed: 0, waiting: 0 };
        for (const child of node.children) for (const key of Object.keys(totals) as (keyof LeafTotals)[]) totals[key] += child.totals![key];
        node.totals = totals;
      }
    } else if (complete) node.totals = { total: 1, done: Number(node.task.status === "done"), held: Number(node.task.status === "held"), dropped: Number(node.task.status === "dropped"),
      claimed: Number(node.task.status === "open" && !!node.task.owner && node.task.lease_until !== null && node.task.lease_until > now),
      waiting: Number(node.task.status === "open" && node.task.blocked_by.length > 0) };
  }
  return { roots, nodes, visible: new Set(overview.tasks.map((task) => task.id)),
    warning: complete ? undefined : "Hierarchy incomplete · group totals unavailable; refresh or inspect history." };
}

export const isGroup = (node: GoalNode): boolean => node.task.children > 0 || node.children.length > 0;
export function groupSummary(node: GoalNode): string {
  const p = node.totals;
  return p ? [`${p.done}/${p.total} done`, p.claimed && `${p.claimed} claimed`, p.held && `${p.held} held`, p.waiting && `${p.waiting} waiting`, p.dropped && `${p.dropped} dropped`].filter(Boolean).join(" · ") : "totals unavailable";
}

export function treeRows(tree: GoalTree, collapsed: ReadonlySet<string> = new Set()): TreeRow[] {
  const result: TreeRow[] = [], seen = new Set<string>();
  const stack = [...tree.roots].reverse().map((node) => ({ node, depth: 0 }));
  while (stack.length) {
    const item = stack.pop()!;
    if (seen.has(item.node.task.id)) continue;
    seen.add(item.node.task.id);
    if (!isGroup(item.node) && !tree.visible.has(item.node.task.id)) continue;
    const expanded = isGroup(item.node) && !collapsed.has(item.node.task.id);
    result.push({ ...item, expanded });
    if (expanded) for (let i = item.node.children.length - 1; i >= 0; i--) stack.push({ node: item.node.children[i]!, depth: item.depth + 1 });
  }
  return result;
}

/** Bounded dashboard paths, never detached leaves with their parent context lost. */
export function previewRows(tree: GoalTree, limit: number): TreeRow[] {
  const eligible = tree.roots.filter((node) => isGroup(node) || tree.visible.has(node.task.id));
  const chosen = eligible.map((node, index) => ({ node, index })).sort((a, b) => a.node.rank - b.node.rank || a.index - b.index)
    .slice(0, limit).sort((a, b) => a.index - b.index);
  const rows: TreeRow[] = [], seen = new Set<string>();
  for (const { node: root } of chosen) {
    let node: GoalNode | undefined = root;
    let depth = 0;
    while (node && !seen.has(node.task.id) && rows.length < 12) {
      seen.add(node.task.id);
      const next: GoalNode | undefined = depth < 3 ? node.children.filter((child) => child.rank <= 1 && (isGroup(child) || tree.visible.has(child.task.id)))
        .sort((a, b) => a.rank - b.rank)[0] : undefined;
      rows.push({ node, depth, expanded: next !== undefined });
      node = next; depth++;
    }
  }
  return rows;
}
