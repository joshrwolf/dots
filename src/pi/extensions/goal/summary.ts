import {truncateToWidth} from '@earendil-works/pi-tui';
import {labelText, type RowModel, type Segment} from '@joshrwolf/pi-kit/ui';
import {
  workerFrame,
  type WorkerActivity,
  type WorkerActivitySnapshot,
} from '../../src/ui/worker-activity.ts';
import type {GoalView} from './present.ts';
import type {GoalTask} from './source.ts';
import {goalTree, isGroup, type GoalTree} from './hierarchy.ts';
const text = (value: string, max = 512) =>
  truncateToWidth(labelText(value), max);
const claimed = (task: GoalTask) =>
  task.owner !== null && (task.status === 'open' || task.status === 'review');

/** Live work on one goal: this runtime's claims and its assigned workers. */
export interface GoalWork {
  readonly mine: readonly GoalTask[];
  readonly workers: readonly {
    readonly worker: WorkerActivity;
    readonly task: GoalTask | undefined;
  }[];
}
/** Only explicit claims and live assignments in this exact project's goal tree qualify. */
export function goalWork(
  view: GoalView,
  activity?: WorkerActivitySnapshot,
  owner?: string,
  tree = view.snapshot && !view.error ? goalTree(view.snapshot) : undefined,
): GoalWork {
  const snapshot = view.snapshot;
  if (!snapshot || !tree || view.error) return {mine: [], workers: []};
  const {root} = snapshot.overview.goal;
  const task = (id: string) =>
    id === root.id ? root : tree.nodes.get(id)?.task;
  return {
    mine: owner
      ? [...tree.nodes.values()]
          .map(node => node.task)
          .filter(task => task.owner === owner && claimed(task))
      : [],
    workers:
      activity?.workers.flatMap(worker =>
        worker.assignment.project === view.focus.project &&
        worker.assignment.project === snapshot.project &&
        (worker.assignment.id === root.id ||
          tree.nodes.has(worker.assignment.id))
          ? [{worker, task: task(worker.assignment.id)}]
          : [],
      ) ?? [],
  };
}

function cue(
  view: GoalView,
  work: GoalWork,
  tree: GoalTree | undefined,
  frame: number,
): string {
  if (view.error) return '!';
  const snapshot = view.snapshot;
  if (!snapshot) return '·';
  const {root} = snapshot.overview.goal;
  const {progress} = snapshot.overview;
  if (root.status === 'done') return '✓';
  if (root.status === 'dropped') return '−';
  if (root.status === 'held' || progress.held) return '!';
  if (work.workers.some(({worker}) => worker.state === 'asking')) return '?';
  if (progress.review) return '◇';
  if (view.brief?.warning || tree?.warning) return '!';
  return work.workers.some(({worker}) => worker.state === 'running')
    ? workerFrame(frame)
    : '·';
}
const attentionRank = (glyph: string) =>
  glyph === '?' ? 0 : glyph === '!' ? 1 : glyph === '◇' ? 2 : 3;

/** The persistent surface: one line for the session's goals, selected goal first. */
export function summaryRow(
  views: readonly GoalView[],
  selected: string | undefined,
  activity?: WorkerActivitySnapshot,
  owner?: string,
): RowModel {
  const current = views.find(view => view.focus.id === selected) ?? views[0];
  if (!current) return {start: [], end: []};
  const frame = activity?.frame ?? 0;
  const tree =
    current.snapshot && !current.error ? goalTree(current.snapshot) : undefined;
  const work = goalWork(current, activity, owner, tree);
  const glyph = cue(current, work, tree, frame);
  const snapshot = current.error ? undefined : current.snapshot;
  const p = snapshot?.overview.progress;
  const status = snapshot?.overview.goal.root.status;
  const attention = current.error
    ? 'unavailable · /goal refresh'
    : !snapshot
      ? 'loading'
      : [
          p?.held && `${p.held} held`,
          p?.review && `${p.review} review`,
          tree?.warning && 'partial',
          current.brief?.warning && 'brief warning',
          (status === 'done' || status === 'dropped') && status,
        ]
          .filter(Boolean)
          .join(' · ');
  // Prefer this runtime's own claim, then a live worker's task, then any claim.
  const now =
    work.mine[0] ??
    work.workers.find(({task}) => task)?.task ??
    [...(tree?.nodes.values() ?? [])].find(
      node => !isGroup(node) && claimed(node.task),
    )?.task;
  const asking = work.workers.filter(
    ({worker}) => worker.state === 'asking',
  ).length;
  const live = work.workers.length;
  const others = views.filter(view => view !== current);
  const urgent = others.some(view => {
    const otherTree =
      view.snapshot && !view.error ? goalTree(view.snapshot) : undefined;
    return (
      attentionRank(
        cue(view, goalWork(view, activity, owner, otherTree), otherTree, 0),
      ) < 2
    );
  });
  const segments: Segment[] = [
    {
      // Review stays on slices and in the attention text, not beside the goal identity.
      text: glyph === '◇' ? '◎' : `◎ ${glyph}`,
      tone: glyph === '!' ? 'warning' : 'accent',
      priority: 100,
      shrink: 'clip',
    },
    {
      text: text(snapshot?.overview.goal.root.title ?? current.focus.id),
      tone: 'text',
      bold: true,
      minWidth: 8,
      priority: 90,
      shrink: 'clip',
    },
  ];
  if (p)
    segments.push({
      text: p.total ? `${p.done}/${p.total} done` : 'no slices',
      tone: 'muted',
      priority: 60,
      shrink: 'omit',
      separatorBefore: ' · ',
    });
  if (attention)
    segments.push({
      text: attention,
      tone: 'warning',
      priority: 80,
      shrink: 'omit',
      separatorBefore: ' · ',
    });
  if (now)
    segments.push({
      text: `now: ${text(now.title)}`,
      tone: 'accent',
      minWidth: 12,
      priority: 70,
      shrink: 'clip',
      separatorBefore: ' · ',
    });
  if (live)
    segments.push({
      text: asking
        ? `${asking} asking${live > asking ? ` · ${live - asking} working` : ''}`
        : `${live} worker${live === 1 ? '' : 's'}`,
      tone: asking ? 'warning' : 'muted',
      priority: 75,
      shrink: 'omit',
      separatorBefore: ' · ',
    });
  if (others.length)
    segments.push({
      text: `+${others.length} more${urgent ? ' · needs attention' : ''}`,
      tone: urgent ? 'warning' : 'dim',
      priority: 40,
      shrink: 'omit',
      separatorBefore: ' · ',
    });
  return {
    separator: ' ',
    start: segments,
    end: [{text: '⌘G', tone: 'dim', priority: 20, shrink: 'omit'}],
  };
}
