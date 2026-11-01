import type {Theme} from '@earendil-works/pi-coding-agent';
import {VStack, truncateToWidth} from '@earendil-works/pi-tui';
import {
  PriorityRow,
  labelText,
  type RowModel,
  type Tone,
} from '@joshrwolf/pi-kit/ui';
import {goalTree, isGroup, type GoalNode} from './hierarchy.ts';
import type {GoalView} from './present.ts';
import type {GoalTask} from './source.ts';
import {
  workerFrame,
  type WorkerActivitySnapshot,
} from '../../src/ui/worker-activity.ts';

const text = (value: string) => truncateToWidth(labelText(value), 512);
const line = (model: RowModel, theme: Theme) => {
  const row = new PriorityRow();
  row.update(model, theme);
  return row;
};

/** Only explicit live assignments to this exact project's goal tree qualify. */
export function goalCue(
  view: GoalView,
  activity?: WorkerActivitySnapshot,
): string {
  if (view.error) return '!';
  const snapshot = view.snapshot;
  if (!snapshot) return '·';
  const {root} = snapshot.overview.goal;
  const {progress} = snapshot.overview;
  if (root.status === 'done') return '✓';
  if (root.status === 'dropped') return '−';
  if (root.status === 'held' || progress.held) return '!';
  const ids = new Set([root.id, ...goalTree(snapshot).nodes.keys()]);
  const matches =
    activity?.workers.filter(
      worker =>
        worker.assignment.project === view.focus.project &&
        worker.assignment.project === snapshot.project &&
        ids.has(worker.assignment.id),
    ) ?? [];
  if (matches.some(worker => worker.state === 'asking')) return '?';
  if (progress.review) return '◇';
  if (view.brief?.warning || goalTree(snapshot).warning) return '!';
  if (matches.some(worker => worker.state === 'quiet')) return '·';
  return matches.some(worker => worker.state === 'running')
    ? workerFrame(activity?.frame ?? 0)
    : '·';
}

function state(
  task: GoalTask,
  now: number,
): {glyph: string; tone: Tone; rank: number} {
  if (task.status === 'held') return {glyph: '!', tone: 'warning', rank: 0};
  if (task.status === 'review') return {glyph: '◇', tone: 'accent', rank: 1};
  if (task.blocked_by.length) return {glyph: '○', tone: 'muted', rank: 2};
  if (task.status === 'open' && task.owner)
    return {glyph: '›', tone: 'accent', rank: 3};
  if (task.status === 'open') return {glyph: '·', tone: 'muted', rank: 4};
  if (task.status === 'done') return {glyph: '✓', tone: 'success', rank: 5};
  return {glyph: '−', tone: 'dim', rank: 6};
}

function path(node: GoalNode): string {
  const titles: string[] = [];
  const seen = new Set<string>();
  let current: GoalNode | undefined = node;
  while (current && !seen.has(current.task.id)) {
    seen.add(current.task.id);
    titles.unshift(text(current.task.title));
    current = current.parent;
  }
  return titles.join(' › ');
}

/** One line per useful slice, with its ancestry kept on that same line. */
function slices(view: GoalView) {
  const tree = goalTree(view.snapshot!);
  return [...tree.nodes.values()]
    .filter(node => tree.visible.has(node.task.id) && !isGroup(node))
    .map((node, index) => ({
      node,
      index,
      rank: state(node.task, view.snapshot!.now).rank,
    }))
    .sort((a, b) => a.rank - b.rank || a.index - b.index);
}

function heading(view: GoalView, theme: Theme, cue: string, selected: boolean) {
  const snapshot = view.error ? undefined : view.snapshot;
  const p = snapshot?.overview.progress;
  const status = snapshot?.overview.goal.root.status;
  const warning = view.error
    ? 'unavailable · refresh'
    : !snapshot
      ? 'loading'
      : [
          goalTree(snapshot).warning && 'partial',
          view.brief?.warning && 'brief warning',
          (status === 'done' || status === 'dropped') && status,
        ]
          .filter(Boolean)
          .join(' · ');
  return line(
    {
      divider: selected,
      start: [
        {
          text: cue === '◇' ? '◎' : `◎ ${cue}`,
          tone: cue === '!' ? 'warning' : 'accent',
          priority: 100,
          shrink: 'clip',
        },
        {
          text: text(snapshot?.overview.goal.root.title ?? view.focus.id),
          tone: 'text',
          bold: selected,
          minWidth: 8,
          priority: 90,
          shrink: 'clip',
        },
        ...(p
          ? [
              {
                text: p.total ? `${p.done}/${p.total}` : 'no slices',
                tone: 'muted' as const,
                priority: 50,
                shrink: 'omit' as const,
              },
            ]
          : []),
        ...(warning
          ? [
              {
                text: warning,
                tone: 'warning' as const,
                priority: 70,
                shrink: 'omit' as const,
              },
            ]
          : []),
      ],
      end: selected
        ? [{text: '/goal', tone: 'muted', priority: 20, shrink: 'omit'}]
        : [],
      separator: ' ',
    },
    theme,
  );
}

/** At most four total rows for one goal; at most six for any followed set. */
export function compactGoals(
  views: readonly GoalView[],
  selected: string | undefined,
  theme: Theme,
  activity?: WorkerActivitySnapshot,
): VStack {
  const result = new VStack();
  if (!views.length) return result;
  const current = views.find(view => view.focus.id === selected) ?? views[0]!;
  result.addChild(heading(current, theme, goalCue(current, activity), true));
  let lines = 1;
  const snapshot = current.error ? undefined : current.snapshot;
  if (snapshot && snapshot.overview.goal.root.status === 'open') {
    const all = slices(current);
    const limit = all.length > 2 ? 2 : Math.min(2, all.length);
    for (const {node} of all.slice(0, limit)) {
      const {glyph, tone} = state(node.task, snapshot.now);
      lines++;
      result.addChild(
        line(
          {
            indent: 2,
            start: [
              {text: glyph, tone, priority: 100, shrink: 'clip'},
              {
                text: path(node),
                tone: 'text',
                priority: 90,
                shrink: 'clip',
                minWidth: 4,
              },
            ],
            end: [],
          },
          theme,
        ),
      );
    }
    const incomplete = goalTree(snapshot).warning;
    if (all.length > limit || incomplete) {
      lines++;
      result.addChild(
        line(
          {
            indent: 2,
            start: [
              {
                text: `${all.length > limit ? `${all.length - limit} more visible slices · ` : ''}${incomplete ? 'hierarchy incomplete · ' : ''}/goal`,
                tone: incomplete ? 'warning' : 'dim',
                priority: 100,
                shrink: 'clip',
              },
            ],
            end: [],
          },
          theme,
        ),
      );
    }
  }
  const rank = (view: GoalView) => {
    const cue = goalCue(view, activity);
    return cue === '?' ? 0 : cue === '!' ? 1 : cue === '◇' ? 2 : 3;
  };
  const others = views
    .filter(view => view !== current)
    .sort((a, b) => rank(a) - rank(b));
  const remaining = Math.max(0, 6 - lines);
  const shown = others.slice(
    0,
    others.length > remaining ? Math.max(0, remaining - 1) : remaining,
  );
  for (const view of shown)
    result.addChild(heading(view, theme, goalCue(view, activity), false));
  if (others.length > shown.length)
    result.addChild(
      line(
        {
          start: [
            {
              text: `+${others.length - shown.length} more followed · /goal`,
              tone: 'dim',
              priority: 100,
              shrink: 'clip',
            },
          ],
          end: [],
        },
        theme,
      ),
    );
  return result;
}
