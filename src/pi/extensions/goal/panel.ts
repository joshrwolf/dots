import type {KeybindingsManager, Theme} from '@earendil-works/pi-coding-agent';
import {
  VStack,
  matchesKey,
  truncateToWidth,
  visibleWidth,
  Text,
  type Component,
  type KeyId,
  type OverlayOptions,
} from '@earendil-works/pi-tui';
import {
  PriorityRow,
  labelText,
  sourceText,
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
  briefNote,
  holdText,
  progressSummary,
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
  // Open, but dots holds it: a request or a held ancestor, not readiness.
  if (task.status === 'open' && task.holds.length)
    return task.holds.some(h => h.kind === 'request' && h.state === 'open')
      ? {label: '? Needs answer', tone: 'warning'}
      : {label: '! Held', tone: 'warning'};
  if (task.owner) return {label: '● Claimed', tone: 'accent', emphasized: true};
  if (task.blocked_by.length) return {label: '◌ Waiting', tone: 'muted'};
  return {label: '○ Open', tone: 'muted'};
}

const glyph = (value: string): Segment => ({
  text: value,
  tone: 'accent',
  bold: true,
  priority: 100,
  shrink: 'clip',
});
/** One selectable task in the list: an outline row, or a graph row with its link lines. */
interface Item {
  readonly task: GoalTask;
  readonly tree?: TreeRow;
  readonly lines?: readonly string[];
}

/**
 * An outline row indents by depth and folds groups; a graph row starts with
 * dots' prefix and names the task's group. Graph link lines follow their row.
 */
function listRow(
  {task, tree, lines = []}: Item,
  group: string | undefined,
  theme: Theme,
  owner: string | undefined,
  selected: boolean,
  width: number,
): string[] {
  const folding = tree && isGroup(tree.node);
  const depth = tree?.depth ?? 0;
  const state = taskState(task);
  const closed = task.status === 'done' || task.status === 'dropped';
  const [prefix = '', ...links] = lines;
  // Graph whitespace locates lanes; it must not pass through label formatting.
  const graphPrefix = tree
    ? ''
    : `${selected ? theme.fg('accent', '→ ') : '  '}${theme.fg('muted', sourceText(prefix).replaceAll('\n', ' '))} `;
  const line = row(
    {
      // The selection marker takes the place of two indent cells.
      indent: tree ? Math.min(depth, 6) * 2 + (selected ? 0 : 2) : 0,
      layout: 'inline',
      groupSeparator: ' · ',
      endSeparator: ' · ',
      start: [
        ...(selected && tree ? [glyph('→')] : []),
        folding
          ? glyph(tree.expanded ? '▾' : '▸')
          : {
              text: tree ? state.label : state.label.slice(2),
              tone: state.tone,
              bold: state.emphasized ?? false,
              width: tree ? 12 : 8,
              minWidth: 1,
              priority: 100,
              shrink: 'clip',
            },
        {
          text: `${depth > 6 ? `[level ${depth + 1}] ` : ''}${text(task.title)}`,
          tone:
            state.emphasized && !folding ? 'accent' : closed ? 'muted' : 'text',
          bold: folding || selected || (state.emphasized ?? false),
          minWidth: 8,
          priority: 80,
          shrink: 'clip',
        },
      ],
      end: [
        {
          text: group ? text(group, 64) : '',
          tone: 'dim',
          priority: 50,
          shrink: 'omit',
        },
        {
          text: folding
            ? groupSummary(tree.node)
            : claimed(task)
              ? task.owner === owner
                ? 'you'
                : text(task.owner!, 64)
              : '',
          tone: tree?.node.totals?.held ? 'warning' : 'muted',
          minWidth: folding ? 8 : 0,
          priority: 70,
          shrink: folding ? 'clip' : 'omit',
        },
      ],
    },
    theme,
  ).render(Math.max(0, width - visibleWidth(graphPrefix)));
  return [
    truncateToWidth(graphPrefix + (line[0] ?? ''), width, ''),
    ...links.map(link =>
      truncateToWidth(
        theme.fg('muted', `  ${sourceText(link).replaceAll('\n', ' ')}`),
        width,
        '',
      ),
    ),
  ];
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
      `Claimed by ${task.owner === model.owner ? 'this Pi runtime' : text(task.owner!, 128)}`,
      'accent',
    ]);
  for (const {worker} of work.workers.filter(({task: t}) => t?.id === task.id))
    lines.push([
      `Worker ${text(worker.label, 64)} · ${worker.state === 'running' ? text(worker.doing ?? 'working', 160) : worker.state}`,
      worker.state === 'asking' ? 'warning' : 'accent',
    ]);
  for (const held of task.holds.slice(0, 4))
    lines.push([`Held: ${holdText(held)}`, 'warning']);
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

/** At most `rows` lines, marking clipped content without padding short content. */
function clip(
  lines: readonly string[],
  rows: number,
  width: number,
  theme: Theme,
): string[] {
  if (rows <= 0) return [];
  if (lines.length <= rows) return [...lines];
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
  private history = true;
  private outline = false;
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

  /** All recorded tasks by default; the filter changes only this popup. */
  private shown(): GoalView | undefined {
    const view = this.model?.view;
    const snapshot = view?.snapshot;
    if (!view || !snapshot || view.error) return view;
    return {
      ...view,
      snapshot: withHierarchy(snapshot, this.history),
      history: this.history,
    };
  }

  private graph(view: GoalView | undefined) {
    const overview = view?.error ? undefined : view?.snapshot?.overview;
    return this.history
      ? overview?.graph
      : (overview?.open_graph ?? overview?.graph);
  }

  /** The dependency graph's rows, or the group outline on request or without a graph. */
  private rows(view: GoalView | undefined): Item[] {
    const snapshot = view?.error ? undefined : view?.snapshot;
    if (!snapshot) return [];
    const graph = this.graph(view);
    if (this.outline || !graph)
      return treeRows(goalTree(snapshot), this.collapsed).map(tree => ({
        task: tree.node.task,
        tree,
      }));
    const tasks = new Map(
      (snapshot.allTasks ?? snapshot.overview.tasks).map(task => [
        task.id,
        task,
      ]),
    );
    return graph.rows.flatMap(({task, lines}) => {
      const found = tasks.get(task);
      return found ? [{task: found, lines}] : [];
    });
  }

  /** The selected row: the chosen task if visible, else this runtime's current work. */
  private index(view: GoalView, rows: readonly Item[]): number {
    const at = rows.findIndex(item => item.task.id === this.selected);
    if (at >= 0) return at;
    const {mine, workers} = goalWork(
      view,
      this.model?.activity,
      this.model?.owner,
    );
    const now = mine[0] ?? workers.find(({task}) => task)?.task;
    return Math.max(
      0,
      rows.findIndex(({task}) => task.id === now?.id),
    );
  }

  setGoal(model: PanelModel, theme: Theme): void {
    const changed = this.model?.view.focus.id !== model.view.focus.id;
    if (changed) {
      this.section = 'overview';
      this.collapsed.clear();
      this.selected = undefined;
      this.history = true;
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
    const graphed = Boolean(this.graph(view));
    const keys = [
      '1 overview · 2 brief · 3 record',
      this.section === 'overview'
        ? `↑/↓ select${this.outline || !graphed ? ' · Space fold' : ''}`
        : '↑/↓ scroll',
      this.section === 'overview' &&
        graphed &&
        (this.outline ? 'o graph' : 'o outline'),
      view.snapshot?.allTasks &&
        (this.history ? 'h hide closed' : 'h show closed'),
      model.followed > 1 && 'Tab goal',
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
        footer: {
          layout: 'inline',
          groupSeparator: ' · ',
          start: [
            {
              text: keys.filter(Boolean).join(' · '),
              tone: 'dim',
              priority: 50,
              shrink: 'clip',
            },
          ],
          end: [
            {
              text: `Esc/${PANEL_KEY} close`,
              tone: 'dim',
              priority: 100,
              shrink: 'clip',
            },
          ],
        },
      },
      theme,
      resetScroll,
    );
  }

  /** The list lines around the selection, with omissions and notes. */
  private window(
    view: GoalView,
    rows: readonly Item[],
    notes: string[],
    warnings: readonly string[],
    height: number,
    width: number,
  ): string[] {
    const theme = this.theme!;
    const selected = this.index(view, rows);
    const owner = this.model?.owner;
    const groups = new Map(
      (view.snapshot?.allTasks ?? view.snapshot?.overview.tasks ?? []).map(
        task => [task.id, task.title],
      ),
    );
    const blocks = rows.map((item, i) =>
      listRow(
        item,
        item.tree || !item.task.parent
          ? undefined
          : groups.get(item.task.parent),
        theme,
        owner,
        i === selected,
        width,
      ),
    );
    const lines = blocks.flat();
    const start = blocks.slice(0, selected).flat().length;
    let space = Math.max(0, height - notes.length);
    if (lines.length > space) space = Math.max(0, space - 1);
    const top = Math.max(
      0,
      Math.min(start - Math.floor(space / 2), lines.length - space),
    );
    const shown = lines.slice(top, top + space);
    return clip(
      [
        ...shown,
        ...(shown.length < lines.length
          ? note(
              `lines ${top + 1}–${top + shown.length} of ${lines.length} · ↑/↓ select`,
              theme,
            ).render(width)
          : []),
        ...notes.flatMap(value =>
          note(
            value,
            theme,
            warnings.includes(value) ? 'warning' : 'dim',
          ).render(width),
        ),
      ],
      height,
      width,
      theme,
    );
  }

  /** Header, then the task list beside (or above) detail, bounded by the available height. */
  private overview(
    view: GoalView,
    rows: readonly Item[],
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
      view.brief?.warning && `${briefNote(view)} · 2 brief`,
    ].filter((value): value is string => Boolean(value));
    const hidden = snapshot.overview.hidden_closed;
    // dots reports what the graph could not draw, such as cycle edges.
    const warnings = [
      tree.warning,
      ...(this.outline ? [] : (this.graph(view)?.notes ?? [])),
    ].filter((value): value is string => Boolean(value));
    const notes = [
      !rows.length &&
        (hidden ? 'No open slices remain.' : 'No slices recorded.'),
      ...warnings,
      hidden && `${hidden} closed hidden · h shows them`,
    ].filter((value): value is string => Boolean(value));
    const split = width >= 100;
    const treeWidth = split ? Math.floor(width * 0.45) : width;
    const detailWidth = split ? width - treeWidth - 3 : width;
    // Tree and detail claim rows first: a heading, up to five tasks with their
    // notes, and when stacked a short detail. The goal result takes what remains,
    // down to one line, then only the record tab shows it.
    const listed = rows.reduce((n, item) => n + (item.lines?.length ?? 1), 0);
    const treeMin =
      1 + Math.min(listed, 5) + notes.length + (listed > 5 ? 1 : 0);
    const reserved = treeMin + (split ? 0 : 4);
    const summary = row(
      {
        layout: 'inline',
        start: [
          {
            text: progressSummary(view),
            tone: 'muted',
            minWidth: 12,
            priority: 70,
            shrink: 'clip',
          },
          ...attention.map(value => ({
            text: value,
            tone: 'warning' as const,
            priority: 90,
            shrink: 'clip' as const,
            separatorBefore: ' · ',
          })),
        ],
        end: [],
      },
      theme,
    ).render(width);
    const spare = height - summary.length - reserved;
    const result = root.result
      ? new MarkdownView(root.result, theme).render(width).filter(Boolean)
      : note('No goal result recorded yet.', theme).render(width);
    const header = [
      ...summary,
      ...(spare >= 2
        ? [
            ...heading(
              'Goal result',
              root.result_event !== null ? `event ${root.result_event}` : '',
              theme,
            ).render(width),
            ...clip(
              result,
              Math.min(4, spare - 1, result.length),
              width,
              theme,
            ),
          ]
        : spare === 1
          ? [
              truncateToWidth(
                theme.fg('accent', 'Goal result ') + (result[0] ?? ''),
                width,
              ),
            ]
          : []),
    ];
    const rest = Math.max(0, height - header.length);
    const selected = rows[this.index(view, rows)]?.task ?? root;
    const described = [
      ...heading('Detail', '', theme).render(detailWidth),
      ...detail(selected, tree, work, model, theme).render(detailWidth),
    ];
    // Stacked, the list keeps most rows; detail gets up to a third, at least
    // four, and any rows the list does not need.
    const treeRows = split
      ? rest
      : Math.max(
          Math.min(treeMin, rest),
          Math.min(
            1 + listed + notes.length,
            rest -
              Math.min(described.length, Math.max(4, Math.floor(rest / 3))),
          ),
        );
    const left = [
      ...heading(
        'Tasks',
        [
          view.history && 'all tasks',
          spare < 1 && root.result && 'goal result: 3 record',
        ]
          .filter(Boolean)
          .join(' · '),
        theme,
      ).render(treeWidth),
      ...this.window(view, rows, notes, warnings, treeRows - 1, treeWidth),
    ];
    const right = clip(
      described,
      split ? rest : rest - left.length,
      detailWidth,
      theme,
    );
    if (!split) return [...header, ...left, ...right].slice(0, height);
    const rule = theme.fg('dim', ' │ ');
    return [
      ...header,
      ...Array.from(
        {length: Math.max(left.length, right.length)},
        (_, i) =>
          truncateToWidth(left[i] ?? '', treeWidth, '', true) +
          rule +
          (right[i] ?? ''),
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
    } else if (
      matchesKey(data, 'o') &&
      this.section === 'overview' &&
      this.graph(view)
    ) {
      this.outline = !this.outline;
      this.refresh();
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

  /** List keys: select; in the outline, Space folds or unfolds a group.
   * Left/Right belong to the popup's views. */
  private move(data: string, view: GoalView, rows: readonly Item[]): boolean {
    const at = this.index(view, rows);
    const {task, tree} = rows[at]!;
    const step = matchesKey(data, 'up') ? -1 : matchesKey(data, 'down') ? 1 : 0;
    this.selected = task.id;
    if (step)
      this.selected =
        rows[Math.max(0, Math.min(rows.length - 1, at + step))]!.task.id;
    else if (tree && isGroup(tree.node) && matchesKey(data, 'space')) {
      if (tree.expanded) this.collapsed.add(task.id);
      else this.collapsed.delete(task.id);
    } else return false;
    return true;
  }
}
