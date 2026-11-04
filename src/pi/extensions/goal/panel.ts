import type {KeybindingsManager, Theme} from '@earendil-works/pi-coding-agent';
import {
  VStack,
  matchesKey,
  truncateToWidth,
  Text,
  type Component,
  type KeyId,
  type OverlayOptions,
} from '@earendil-works/pi-tui';
import {
  PriorityRow,
  labelText,
  type RowModel,
  type Segment,
  type Tone,
} from '@joshrwolf/pi-kit/ui';
import {MarkdownView} from '../../src/ui/markdown.ts';
import {Inspector} from '../../src/ui/inspector.ts';
import {OutputView} from '../../src/ui/card.ts';
import {
  workerFrame,
  type WorkerActivity,
  type WorkerActivitySnapshot,
} from '../../src/ui/worker-activity.ts';
import {
  artifacts,
  compactSummary,
  inspection,
  latestEvent,
  type GoalView,
} from './present.ts';
import {withHierarchy, type GoalTask} from './source.ts';
import {
  goalTree,
  groupSummary,
  isGroup,
  treeRows,
  type GoalTree,
  type TreeRow,
} from './hierarchy.ts';

/** Opens the goal popup from the editor; closes it from the popup. */
export const PANEL_KEY: KeyId = 'alt+g';

/** A large centered popup; minWidth fills narrow terminals. */
export const panelOverlay = {
  anchor: 'center',
  width: '90%',
  minWidth: 80,
  maxHeight: '100%',
} satisfies OverlayOptions;
/** The popup's columns for a terminal width, as Pi resolves panelOverlay. */
export const panelWidth = (columns: number): number =>
  Math.min(columns, Math.max(80, Math.floor(columns * 0.9)));
/** Body rows: 85% of the screen, or all of a short one, less the frame's heading and footer. */
export const panelHeight = (rows: number): number =>
  Math.max(1, (rows < 30 ? rows : Math.floor(rows * 0.85)) - 2);

const text = (value: string, max = 512) =>
  truncateToWidth(labelText(value), max);
const row = (model: RowModel, theme: Theme): PriorityRow => {
  const view = new PriorityRow();
  view.update(model, theme);
  return view;
};
const note = (value: string, theme: Theme, tone: Tone = 'dim') =>
  row(
    {
      start: [{text: value, tone, priority: 100, shrink: 'clip'}],
      end: [],
    },
    theme,
  );
const heading = (title: string, detail: string, theme: Theme) =>
  row(
    {
      divider: true,
      start: [
        {
          text: title,
          tone: 'accent',
          bold: true,
          priority: 100,
          shrink: 'clip',
        },
      ],
      end: detail
        ? [{text: detail, tone: 'muted', priority: 50, shrink: 'omit'}]
        : [],
    },
    theme,
  );

const claimed = (task: GoalTask) =>
  task.owner !== null && (task.status === 'open' || task.status === 'review');

/** Live work on one goal: this session's claims and its assigned workers. */
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

/** The persistent surface: one line for the followed set, selected goal first. */
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
  // Prefer this session's own claim, then a live worker's task, then any claim.
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
      text: `+${others.length} followed${urgent ? ' · needs attention' : ''}`,
      tone: urgent ? 'warning' : 'dim',
      priority: 40,
      shrink: 'omit',
      separatorBefore: ' · ',
    });
  return {
    separator: ' ',
    start: segments,
    end: [{text: PANEL_KEY, tone: 'dim', priority: 20, shrink: 'omit'}],
  };
}

type TaskState = {label: string; tone: Tone; emphasized?: boolean};
function taskState(task: GoalTask): TaskState {
  if (task.status === 'done') return {label: '✓ Done', tone: 'success'};
  if (task.status === 'held') return {label: '! Held', tone: 'warning'};
  if (task.status === 'dropped') return {label: '− Dropped', tone: 'dim'};
  if (task.status === 'review')
    return {label: '◇ Review', tone: 'accent', emphasized: true};
  if (task.owner) return {label: '› Claimed', tone: 'accent', emphasized: true};
  if (task.blocked_by.length) return {label: '· Waiting', tone: 'muted'};
  return {label: '· Open', tone: 'muted'};
}

const glyph = (value: string): Segment => ({
  text: value,
  tone: 'accent',
  bold: true,
  priority: 100,
  shrink: 'clip',
});
function treeRow(
  item: TreeRow,
  theme: Theme,
  owner: string | undefined,
  selected: boolean,
): Component {
  const {node, depth, expanded} = item;
  const group = isGroup(node);
  const state = taskState(node.task);
  const closed = node.task.status === 'done' || node.task.status === 'dropped';
  return row(
    {
      // The selection marker takes the place of two indent cells.
      indent: Math.min(depth, 6) * 2 + (selected ? 0 : 2),
      layout: 'inline',
      groupSeparator: ' · ',
      start: [
        ...(selected ? [glyph('→')] : []),
        group
          ? glyph(expanded ? '▾' : '▸')
          : {
              text: state.label,
              tone: state.tone,
              bold: state.emphasized ?? false,
              width: 12,
              minWidth: 1,
              priority: 100,
              shrink: 'clip',
            },
        {
          text: `${depth > 6 ? `[level ${depth + 1}] ` : ''}${text(node.task.title)}`,
          tone:
            state.emphasized && !group ? 'accent' : closed ? 'muted' : 'text',
          bold: group || selected || (state.emphasized ?? false),
          minWidth: 8,
          priority: 80,
          shrink: 'clip',
        },
      ],
      end: [
        {
          text: group
            ? groupSummary(node)
            : claimed(node.task)
              ? node.task.owner === owner
                ? 'you'
                : text(node.task.owner!, 64)
              : '',
          tone: node.totals?.held ? 'warning' : 'muted',
          minWidth: group ? 8 : 0,
          priority: 90,
          shrink: group ? 'clip' : 'omit',
        },
      ],
    },
    theme,
  );
}

const wrapped = (value: string, theme: Theme, tone: Tone = 'text') =>
  new Text(theme.fg(tone, value), 0, 0);

/** Everything recorded about one task, plus live workers assigned to it. */
function detail(
  task: GoalTask,
  tree: GoalTree,
  work: GoalWork,
  model: PanelModel,
  theme: Theme,
): Component {
  const state = taskState(task);
  const node = tree.nodes.get(task.id);
  const lines: [string, Tone][] = [
    [theme.bold(labelText(task.title)), 'text'],
    [
      node && isGroup(node)
        ? `${task.status} · ${groupSummary(node)}`
        : state.label,
      state.tone,
    ],
  ];
  if (claimed(task))
    lines.push([
      `Claimed by ${task.owner === model.owner ? 'this session' : text(task.owner!, 128)}`,
      'accent',
    ]);
  for (const {worker} of work.workers.filter(({task: t}) => t?.id === task.id))
    lines.push([
      `Worker ${text(worker.label, 64)} · ${worker.state === 'running' ? text(worker.doing ?? 'working', 160) : worker.state}`,
      worker.state === 'asking' ? 'warning' : 'accent',
    ]);
  for (const id of task.blocked_by.slice(0, 8)) {
    const blocker = tree.nodes.get(id)?.task;
    lines.push([
      `Needs: ${blocker ? `${text(blocker.title)} · ${blocker.status}` : id}`,
      blocker?.status === 'done' ? 'muted' : 'warning',
    ]);
  }
  if (task.blocked_by.length > 8)
    lines.push([`Needs ${task.blocked_by.length - 8} more`, 'warning']);
  if (task.branch) lines.push([`Branch ${text(task.branch)}`, 'muted']);
  for (const line of latestEvent(task)) lines.push([line.trim(), 'muted']);
  // References and evidence as the record states them; the result renders below.
  for (const line of artifacts(task, 0))
    if (!/^\[?Result/.test(line)) lines.push([line, 'muted']);
  const body = new VStack(
    lines.map(([value, tone]) => wrapped(value, theme, tone)),
  );
  body.addChild(
    heading(
      'Result',
      task.result_event !== null
        ? `/goal evidence ${task.id} ${task.result_event}`
        : '',
      theme,
    ),
  );
  body.addChild(
    task.result
      ? new MarkdownView(task.result, theme)
      : note('No result recorded.', theme),
  );
  return body;
}

/** Exactly `rows` lines of `lines`, marking clipped content. */
function clip(
  lines: readonly string[],
  rows: number,
  width: number,
  theme: Theme,
): string[] {
  if (rows <= 0) return [];
  if (lines.length <= rows)
    return [...lines, ...Array<string>(rows - lines.length).fill('')];
  return [
    ...lines.slice(0, rows - 1),
    truncateToWidth(
      theme.fg('dim', `… ${lines.length - rows + 1} more lines · 3 record`),
      width,
    ),
  ];
}

export interface PanelModel {
  readonly view: GoalView;
  readonly activity?: WorkerActivitySnapshot | undefined;
  /** This session's claim owner, to tell its own claims from others'. */
  readonly owner?: string | undefined;
  readonly followed: number;
}
export interface PanelActions {
  readonly close: () => void;
  /** Select the next followed goal. */
  readonly cycle: () => void;
}

/** Focused goal popup: overview with tree and detail, brief, and full record. */
export class GoalPanel extends Inspector {
  private model: PanelModel | undefined;
  private theme: Theme | undefined;
  private section: 'overview' | 'brief' | 'record' = 'overview';
  private history = false;
  private readonly collapsed = new Set<string>();
  private selected: string | undefined;
  private readonly actions: PanelActions;
  private readonly requestRender: () => void;
  private readonly height: () => number;

  constructor(
    bindings: Pick<KeybindingsManager, 'matches'>,
    actions: PanelActions,
    redraw: () => void,
    height: () => number,
  ) {
    super(bindings, actions.close, redraw, height);
    this.actions = actions;
    this.requestRender = redraw;
    this.height = height;
  }

  override render(width: number): string[] {
    return width > 0 ? super.render(width) : [];
  }

  /** The displayed snapshot: closed tasks join only on request or for a closed goal. */
  private shown(): GoalView | undefined {
    const view = this.model?.view;
    const snapshot = view?.snapshot;
    if (!view || !snapshot || view.error || !this.history) return view;
    if (!snapshot.allTasks && snapshot.overview.hidden_closed) return view;
    return {...view, snapshot: withHierarchy(snapshot, true), history: true};
  }

  private rows(view: GoalView | undefined): TreeRow[] {
    return view?.snapshot && !view.error
      ? treeRows(goalTree(view.snapshot), this.collapsed)
      : [];
  }

  /** The selected row: the chosen task if visible, else this session's current work. */
  private index(view: GoalView, rows: readonly TreeRow[]): number {
    const at = rows.findIndex(item => item.node.task.id === this.selected);
    if (at >= 0) return at;
    const {mine, workers} = goalWork(
      view,
      this.model?.activity,
      this.model?.owner,
    );
    const now = mine[0] ?? workers.find(({task}) => task)?.task;
    return Math.max(
      0,
      rows.findIndex(({node}) => node.task === now),
    );
  }

  setGoal(model: PanelModel, theme: Theme): void {
    const changed = this.model?.view.focus.id !== model.view.focus.id;
    const status = model.view.snapshot?.overview.goal.root.status;
    if (changed) {
      this.section = 'overview';
      this.collapsed.clear();
      this.selected = undefined;
      this.history = status === 'done' || status === 'dropped';
    }
    this.model = model;
    this.theme = theme;
    this.refresh(changed);
    this.requestRender();
  }

  private refresh(resetScroll = false): void {
    const model = this.model;
    const theme = this.theme;
    const view = this.shown();
    if (!model || !theme || !view) return;
    const rows = this.rows(view);
    const body =
      this.section === 'overview'
        ? {
            render: (width: number) => this.overview(view, rows, width),
            invalidate() {},
          }
        : this.section === 'brief'
          ? new MarkdownView(
              view.error ??
                [view.brief?.text ?? 'Brief not loaded.', view.brief?.warning]
                  .filter(Boolean)
                  .join('\n\n'),
              theme,
            )
          : new OutputView(inspection(view));
    const keys = [
      '1 overview · 2 brief · 3 record',
      this.section === 'overview' ? '↑/↓ select · ←/→ fold' : '↑/↓ scroll',
      view.snapshot?.allTasks && 'h closed',
      model.followed > 1 && 'Tab next goal',
      `Esc/${PANEL_KEY} close`,
    ];
    this.setContent(
      {
        title: view.snapshot?.overview.goal.root.title ?? view.focus.id,
        detail: view.error
          ? 'unavailable'
          : [
              view.snapshot?.overview.goal.root.status,
              this.section,
              'recorded state, not live',
            ]
              .filter(Boolean)
              .join(' · '),
        body,
        hint: keys.filter(Boolean).join(' · '),
      },
      theme,
      resetScroll,
    );
  }

  /** The tree rows around the selection, with omissions and hierarchy notes. */
  private window(
    view: GoalView,
    tree: GoalTree,
    rows: readonly TreeRow[],
    height: number,
    width: number,
  ): string[] {
    const theme = this.theme!;
    const selected = this.index(view, rows);
    const hidden = view.snapshot?.overview.hidden_closed;
    const notes = [
      !rows.length &&
        (hidden ? 'No open slices remain.' : 'No slices recorded.'),
      tree.warning,
      hidden && `${hidden} closed hidden · h shows them`,
    ].filter((value): value is string => Boolean(value));
    let space = Math.max(0, height - notes.length);
    if (rows.length > space) {
      space = Math.max(0, space - 1);
      notes.unshift(
        `rows ${Math.min(rows.length, space)} of ${rows.length} · ↑/↓ select`,
      );
    }
    const top = Math.max(
      0,
      Math.min(selected - Math.floor(space / 2), rows.length - space),
    );
    return clip(
      [
        ...rows
          .slice(top, top + space)
          .flatMap((item, i) =>
            treeRow(
              item,
              theme,
              this.model?.owner,
              top + i === selected,
            ).render(width),
          ),
        ...notes.flatMap(value =>
          note(value, theme, value === tree.warning ? 'warning' : 'dim').render(
            width,
          ),
        ),
      ],
      height,
      width,
      theme,
    );
  }

  /** Header, then the task tree beside (or above) the selected task's detail, in exactly `height` lines. */
  private overview(
    view: GoalView,
    rows: readonly TreeRow[],
    width: number,
  ): string[] {
    const model = this.model!;
    const theme = this.theme!;
    const height = Math.max(1, this.height());
    const snapshot = view.snapshot;
    if (width <= 0) return [];
    if (view.error || !snapshot)
      return clip(
        wrapped(
          view.error
            ? `Unavailable · /goal refresh\n${text(view.error)}`
            : 'Loading goal…',
          theme,
          view.error ? 'warning' : 'muted',
        ).render(width),
        height,
        width,
        theme,
      );
    const tree = goalTree(snapshot);
    const work = goalWork(view, model.activity, model.owner, tree);
    const {root} = snapshot.overview.goal;
    const asking = work.workers.filter(
      ({worker}) => worker.state === 'asking',
    ).length;
    const attention = [
      tree.warning && 'partial tree',
      work.workers.length &&
        `${work.workers.length} worker${work.workers.length === 1 ? '' : 's'}${asking ? ` · ${asking} asking` : ''}`,
      view.brief?.warning && text(view.brief.warning),
    ].filter(Boolean);
    const header = [
      ...wrapped(
        theme.fg('muted', compactSummary(view)) +
          theme.fg('warning', attention.map(value => ` · ${value}`).join('')),
        theme,
      ).render(width),
      ...heading(
        'Goal result',
        root.result_event !== null ? `event ${root.result_event}` : '',
        theme,
      ).render(width),
      ...clip(
        (root.result
          ? new MarkdownView(root.result, theme)
          : note('No goal result recorded yet.', theme)
        ).render(width),
        Math.min(4, Math.max(1, height - 8)),
        width,
        theme,
      ).filter((line, i) => i === 0 || line),
    ];
    const rest = Math.max(0, height - header.length);
    const split = width >= 100;
    const treeWidth = split ? Math.floor(width * 0.45) : width;
    const detailWidth = split ? width - treeWidth - 3 : width;
    const left = [
      ...heading('Tasks', view.history ? 'with closed' : '', theme).render(
        treeWidth,
      ),
      ...this.window(
        view,
        tree,
        rows,
        (split ? rest : Math.max(2, Math.floor(rest / 2))) - 1,
        treeWidth,
      ),
    ];
    const right = clip(
      [
        ...heading('Detail', '', theme).render(detailWidth),
        ...detail(
          rows[this.index(view, rows)]?.node.task ?? root,
          tree,
          work,
          model,
          theme,
        ).render(detailWidth),
      ],
      split ? rest : rest - left.length,
      detailWidth,
      theme,
    );
    if (!split) return [...header, ...left, ...right].slice(0, height);
    const rule = theme.fg('dim', ' │ ');
    return [
      ...header,
      ...left.map(
        (line, i) =>
          truncateToWidth(line, treeWidth, '', true) + rule + (right[i] ?? ''),
      ),
    ].slice(0, height);
  }

  override handleInput(data: string): void {
    if (matchesKey(data, PANEL_KEY)) {
      this.actions.close();
      return;
    }
    const section = matchesKey(data, '1')
      ? 'overview'
      : matchesKey(data, '2')
        ? 'brief'
        : matchesKey(data, '3')
          ? 'record'
          : undefined;
    const view = this.shown();
    const rows = this.rows(view);
    if (section) {
      this.section = section;
      this.refresh(true);
    } else if (matchesKey(data, 'h') && this.model?.view.snapshot?.allTasks) {
      this.history = !this.history;
      this.refresh();
    } else if (matchesKey(data, 'tab') && (this.model?.followed ?? 0) > 1) {
      this.actions.cycle();
      return;
    } else if (
      this.section === 'overview' &&
      view &&
      rows.length &&
      this.move(data, view, rows)
    ) {
      this.refresh();
    } else {
      super.handleInput(data);
      return;
    }
    this.requestRender();
  }

  /** Tree keys: select, fold, and step to a leaf's parent. */
  private move(
    data: string,
    view: GoalView,
    rows: readonly TreeRow[],
  ): boolean {
    const at = this.index(view, rows);
    const item = rows[at]!;
    const step = matchesKey(data, 'up') ? -1 : matchesKey(data, 'down') ? 1 : 0;
    const id = item.node.task.id;
    this.selected = id;
    if (step)
      this.selected =
        rows[Math.max(0, Math.min(rows.length - 1, at + step))]!.node.task.id;
    else if (
      isGroup(item.node) &&
      (matchesKey(data, 'left') || (matchesKey(data, 'space') && item.expanded))
    )
      this.collapsed.add(id);
    else if (
      isGroup(item.node) &&
      (matchesKey(data, 'right') || matchesKey(data, 'space'))
    )
      this.collapsed.delete(id);
    else if (
      matchesKey(data, 'left') &&
      item.node.task.parent &&
      rows.some(r => r.node.task.id === item.node.task.parent)
    )
      this.selected = item.node.task.parent;
    else return false;
    return true;
  }
}
