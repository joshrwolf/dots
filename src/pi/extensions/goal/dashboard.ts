import type {Theme} from '@earendil-works/pi-coding-agent';
import {
  Spacer,
  VStack,
  truncateToWidth,
  type Component,
} from '@earendil-works/pi-tui';
import {
  PriorityRow,
  type RowModel,
  type Tone,
  labelText,
} from '@joshrwolf/pi-kit/ui';
import type {GoalView} from './present.ts';
import {compactGoals} from './compact.ts';
import type {WorkerActivitySnapshot} from '../../src/ui/worker-activity.ts';
import type {GoalTask} from './source.ts';
import {
  goalTree,
  groupSummary,
  isGroup,
  previewRows,
  treeRows,
  type TreeRow,
  type TreeState,
} from './hierarchy.ts';

const text = (value: string, max = 512) =>
  truncateToWidth(labelText(value), max);
const row = (model: RowModel, theme: Theme): PriorityRow => {
  const view = new PriorityRow();
  view.update(model, theme);
  return view;
};
const note = (value: string, theme: Theme, tone: Tone = 'dim') =>
  row(
    {start: [{text: value, tone, priority: 100, shrink: 'clip'}], end: []},
    theme,
  );

export function compactSummary(view: GoalView): string {
  const p = view.snapshot?.overview.progress;
  if (!p || view.error)
    return view.error ? 'unavailable · /goal refresh' : 'loading';
  return [
    p.total ? `${p.done}/${p.total} slices done` : 'No slices recorded',
    p.claimed && `${p.claimed} claimed`,
    p.held && `${p.held} held`,
    p.review && `${p.review} review`,
    p.dropped && `${p.dropped} dropped`,
    view.brief?.warning && 'brief needs attention',
  ]
    .filter(Boolean)
    .join(' · ');
}

function progress(view: GoalView, theme: Theme, details: boolean): Component {
  const p = view.snapshot?.overview.progress;
  const filled =
    p && p.total > 0 ? Math.min(10, Math.floor((10 * p.done) / p.total)) : 0;
  return row(
    {
      start: [
        {
          text: compactSummary(view),
          tone: view.error || view.brief?.warning ? 'warning' : 'text',
          priority: 100,
          shrink: 'clip',
        },
      ],
      end:
        details && p && p.total > 0 && !view.error
          ? [
              {
                text: `[${'█'.repeat(filled)}${'·'.repeat(10 - filled)}]`,
                tone: p.done === p.total ? 'success' : 'accent',
                priority: 0,
                shrink: 'omit',
              },
            ]
          : [],
    },
    theme,
  );
}

type TaskState = {label: string; tone: Tone; emphasized?: boolean};
function taskState(task: GoalTask, now: number): TaskState {
  if (task.status === 'done') return {label: '✓ Done', tone: 'success'};
  if (task.status === 'held') return {label: '! Held', tone: 'warning'};
  if (task.status === 'dropped') return {label: '− Dropped', tone: 'dim'};
  if (task.status === 'review')
    return {label: '◇ Review', tone: 'accent', emphasized: true};
  if (task.owner) return {label: '› Claimed', tone: 'accent', emphasized: true};
  if (task.blocked_by.length) return {label: '· Waiting', tone: 'muted'};
  return {label: '· Open', tone: 'muted'};
}

function taskRow(
  task: GoalTask,
  view: GoalView,
  theme: Theme,
  depth = 0,
): Component {
  const state = taskState(task, view.snapshot!.now);
  return row(
    {
      indent: Math.min(depth, 6) * 2,
      layout: 'inline',
      start: [
        {
          text: state.label,
          tone: state.tone,
          bold: state.emphasized ?? false,
          width: 12,
          minWidth: 1,
          priority: 100,
          shrink: 'clip',
        },
        {
          text: `${depth > 6 ? `[level ${depth + 1}] ` : ''}${text(task.title)}`,
          tone: state.emphasized
            ? 'accent'
            : task.status === 'done' || task.status === 'dropped'
              ? 'muted'
              : 'text',
          bold: state.emphasized ?? false,
          minWidth: 8,
          priority: 80,
          shrink: 'clip',
        },
      ],
      end: [],
    },
    theme,
  );
}

function hierarchyRow(
  item: TreeRow,
  view: GoalView,
  theme: Theme,
  selected?: string,
): Component {
  const {node, depth, expanded} = item;
  if (!isGroup(node)) return taskRow(node.task, view, theme, depth);
  return row(
    {
      indent: Math.min(depth, 6) * 2,
      layout: 'inline',
      groupSeparator: ' · ',
      start: [
        {
          text: `${node.task.id === selected ? '› ' : ''}${expanded ? '▾' : '▸'}`,
          tone: 'accent',
          width: 3,
          priority: 100,
          shrink: 'clip',
        },
        {
          text: `${depth > 6 ? `[level ${depth + 1}] ` : ''}${text(node.task.title)}`,
          tone: 'text',
          bold: true,
          minWidth: 8,
          priority: 80,
          shrink: 'clip',
        },
      ],
      end: [
        {
          text: node.task.status === 'open' ? '' : node.task.status,
          tone: node.task.status === 'held' ? 'warning' : 'muted',
          priority: 95,
          shrink: 'clip',
        },
        {
          text: groupSummary(node),
          tone: node.totals?.held
            ? 'warning'
            : node.totals?.claimed
              ? 'accent'
              : 'muted',
          minWidth: 8,
          priority: 90,
          shrink: 'clip',
        },
      ],
    },
    theme,
  );
}

function body(
  view: GoalView,
  theme: Theme,
  limit: number,
  details = false,
  state?: TreeState,
): VStack {
  const result = new VStack();
  if (view.error || !view.snapshot) {
    result.addChild(
      note(
        view.error ? 'Unavailable · /goal refresh' : 'Loading goal…',
        theme,
        view.error ? 'warning' : 'muted',
      ),
    );
    if (view.error) result.addChild(note(text(view.error), theme, 'warning'));
    return result; // Never decorate a stale snapshot as current progress.
  }
  const {hidden_closed, goal} = view.snapshot.overview;
  result.addChild(progress(view, theme, details));
  if (goal.root.status !== 'open')
    result.addChild(
      note(
        `Goal: ${goal.root.status}`,
        theme,
        goal.root.status === 'held' ? 'warning' : 'muted',
      ),
    );
  if (details) result.addChild(new Spacer(1));
  const tree = goalTree(view.snapshot);
  const available = treeRows(tree, details ? state?.collapsed : undefined);
  const selected = details
    ? available.slice(0, limit)
    : previewRows(tree, limit);
  const grouped = [...tree.nodes.values()].some(isGroup);
  const labels = new Map(
    [...tree.nodes.values()].map(({task}) => [task.id, text(task.title)]),
  );
  for (const item of selected) {
    const task = item.node.task;
    result.addChild(
      hierarchyRow(item, view, theme, details ? state?.selected : undefined),
    );
    if (details && task.blocked_by.length)
      result.addChild(
        row(
          {
            indent: Math.min(item.depth, 6) * 2 + 13,
            start: [
              {
                text: `Needs: ${task.blocked_by
                  .slice(0, 8)
                  .map(id => labels.get(id) ?? text(id))
                  .join(
                    ', ',
                  )}${task.blocked_by.length > 8 ? ` · +${task.blocked_by.length - 8} more` : ''}`,
                tone: 'muted',
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
  if (!selected.length)
    result.addChild(
      note(
        hidden_closed ? 'No open slices remain.' : 'No slices recorded.',
        theme,
        'muted',
      ),
    );
  const omitted = available.length - selected.length;
  if (omitted > 0)
    result.addChild(
      note(
        `${omitted} more ${details ? 'tasks · 3 details for the full-record command' : grouped ? 'rows · /goal to inspect' : 'slices · /goal to inspect'}`,
        theme,
      ),
    );
  if (tree.warning) result.addChild(note(tree.warning, theme, 'warning'));
  if (hidden_closed)
    result.addChild(
      note(
        `${hidden_closed} closed task details hidden · /goal history`,
        theme,
      ),
    );
  if (view.brief?.warning)
    result.addChild(note(text(view.brief.warning), theme, 'warning'));
  return result;
}

/** An unframed section for Inspector's single scrollable body. */
class GoalInspectionBody extends VStack {
  override render(width: number): string[] {
    return width > 0 ? super.render(width) : [];
  }
}
export function goalOverview(
  view: GoalView,
  theme: Theme,
  state?: TreeState,
): VStack {
  const section = new GoalInspectionBody();
  section.addChild(
    row(
      {
        start: [
          {
            text: 'Slices',
            tone: 'accent',
            bold: true,
            priority: 100,
            shrink: 'clip',
          },
        ],
        end: [],
      },
      theme,
    ),
  );
  section.addChild(body(view, theme, 100, true, state));
  section.addChild(
    row(
      {
        start: [
          {
            text: 'Recorded progress · not live execution',
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
  return section;
}

export class GoalWidget extends VStack {
  override render(width: number): string[] {
    return width > 0 ? super.render(width) : [];
  }
  update(
    view: GoalView,
    theme: Theme,
    activity?: WorkerActivitySnapshot,
  ): void {
    this.clear();
    this.addChild(compactGoals([view], view.focus.id, theme, activity));
    this.invalidate();
  }
}

/** Selected goal gets bounded detail; other follows remain compact. */
export class FollowedWidget extends VStack {
  override render(width: number): string[] {
    return width > 0 ? super.render(width) : [];
  }
  update(
    views: readonly GoalView[],
    selected: string | undefined,
    theme: Theme,
    activity?: WorkerActivitySnapshot,
  ): void {
    this.clear();
    this.addChild(compactGoals(views, selected, theme, activity));
    this.invalidate();
  }
}
