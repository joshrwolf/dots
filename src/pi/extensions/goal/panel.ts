import type {KeybindingsManager, Theme} from '@earendil-works/pi-coding-agent';
import {
  VStack,
  matchesKey,
  truncateToWidth,
  type Component,
  type Focusable,
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
import {compactSummary, inspection, type GoalView} from './present.ts';
import {withHierarchy, type GoalTask} from './source.ts';
import {
  goalTree,
  groupSummary,
  isGroup,
  treeRows,
  type GoalTree,
  type TreeRow,
} from './hierarchy.ts';

/** Toggles the panel from the editor; closes it from the focused panel. */
export const PANEL_KEY: KeyId = 'alt+g';

/** A side panel at the top right; narrow terminals reach full width through minWidth. */
export const panelOverlay = {
  anchor: 'top-right',
  width: '45%',
  minWidth: 64,
  maxHeight: '70%',
  margin: {top: 1, right: 1},
  nonCapturing: true,
} satisfies OverlayOptions;
/** Body rows: the frame reserves two lines for its heading and footer. */
export const panelHeight = (rows: number): number =>
  Math.max(1, Math.floor(rows * 0.7) - 2);

const text = (value: string, max = 512) =>
  truncateToWidth(labelText(value), max);
const row = (model: RowModel, theme: Theme): PriorityRow => {
  const view = new PriorityRow();
  view.update(model, theme);
  return view;
};
const note = (value: string, theme: Theme, tone: Tone = 'dim', indent = 0) =>
  row(
    {
      indent,
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
          .filter(
            task =>
              task.owner === owner &&
              (task.status === 'open' || task.status === 'review'),
          )
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
      node =>
        !isGroup(node) &&
        node.task.owner &&
        (node.task.status === 'open' || node.task.status === 'review'),
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

function treeRow(
  item: TreeRow,
  theme: Theme,
  owner: string | undefined,
  selected: string | undefined,
): Component {
  const {node, depth, expanded} = item;
  const indent = Math.min(depth, 6) * 2;
  const title = `${depth > 6 ? `[level ${depth + 1}] ` : ''}${text(node.task.title)}`;
  if (isGroup(node))
    return row(
      {
        indent,
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
            text: title,
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
  const state = taskState(node.task);
  const claimed =
    node.task.owner &&
    (node.task.status === 'open' || node.task.status === 'review');
  return row(
    {
      indent,
      layout: 'inline',
      groupSeparator: ' · ',
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
          text: title,
          tone: state.emphasized
            ? 'accent'
            : node.task.status === 'done' || node.task.status === 'dropped'
              ? 'muted'
              : 'text',
          bold: state.emphasized ?? false,
          minWidth: 8,
          priority: 80,
          shrink: 'clip',
        },
      ],
      end: claimed
        ? [
            {
              text:
                node.task.owner === owner ? 'you' : text(node.task.owner!, 64),
              tone: 'muted',
              priority: 60,
              shrink: 'omit',
            },
          ]
        : [],
    },
    theme,
  );
}

export type PanelSection = 'overview' | 'brief' | 'record';
export interface PanelModel {
  readonly view: GoalView;
  readonly activity?: WorkerActivitySnapshot | undefined;
  /** This session's claim owner, to tell its own claims from others'. */
  readonly owner?: string | undefined;
  readonly followed: number;
}
export interface PanelActions {
  /** Return input to the editor and keep the panel visible. */
  readonly release: () => void;
  readonly close: () => void;
  /** Select the next followed goal. */
  readonly cycle: () => void;
}

/** Bounded overview: live work, the task tree, and the goal's current result. */
class PanelBody extends VStack {
  override render(width: number): string[] {
    return width > 0 ? super.render(width) : [];
  }
}
function overview(
  model: PanelModel,
  view: GoalView,
  theme: Theme,
  rows: readonly TreeRow[],
  selected: string | undefined,
): {body: VStack; treeTop: number} {
  const body = new PanelBody();
  const snapshot = view.snapshot;
  if (view.error || !snapshot) {
    body.addChild(
      note(
        view.error ? 'Unavailable · /goal refresh' : 'Loading goal…',
        theme,
        view.error ? 'warning' : 'muted',
      ),
    );
    if (view.error) body.addChild(note(text(view.error), theme, 'warning'));
    return {body, treeTop: 0};
  }
  const tree = goalTree(snapshot);
  const work = goalWork(view, model.activity, model.owner, tree);
  const frame = model.activity?.frame ?? 0;
  body.addChild(heading('Now', '', theme));
  for (const task of work.mine)
    body.addChild(
      row(
        {
          indent: 2,
          start: [
            {text: '›', tone: 'accent', priority: 100, shrink: 'clip'},
            {
              text: 'you',
              tone: 'accent',
              bold: true,
              priority: 90,
              shrink: 'clip',
            },
            {
              text: text(task.title),
              tone: 'text',
              minWidth: 8,
              priority: 80,
              shrink: 'clip',
              separatorBefore: ' · ',
            },
          ],
          end: [],
          separator: ' ',
        },
        theme,
      ),
    );
  for (const {worker, task} of work.workers)
    body.addChild(
      row(
        {
          indent: 2,
          start: [
            {
              text:
                worker.state === 'asking'
                  ? '?'
                  : worker.state === 'quiet'
                    ? '·'
                    : workerFrame(frame),
              tone: worker.state === 'asking' ? 'warning' : 'accent',
              priority: 100,
              shrink: 'clip',
            },
            {
              text: text(worker.label, 64),
              tone: 'text',
              bold: true,
              minWidth: 4,
              priority: 90,
              shrink: 'clip',
            },
            {
              text: text(task?.title ?? worker.assignment.id),
              tone: 'muted',
              minWidth: 8,
              priority: 70,
              shrink: 'clip',
              separatorBefore: ' · ',
            },
            {
              text:
                worker.state === 'asking'
                  ? 'asking'
                  : worker.state === 'quiet'
                    ? 'quiet'
                    : text(worker.doing ?? 'working', 160),
              tone: worker.state === 'asking' ? 'warning' : 'dim',
              priority: 60,
              shrink: 'clip',
              separatorBefore: ' · ',
            },
          ],
          end: [],
          separator: ' ',
        },
        theme,
      ),
    );
  if (!work.mine.length && !work.workers.length)
    body.addChild(
      note(
        'No claims or live workers from this session on this goal.',
        theme,
        'dim',
        2,
      ),
    );
  body.addChild(heading('Tasks', compactSummary(view), theme));
  const treeTop = body.children.length;
  const labels = new Map(
    [...tree.nodes.values()].map(({task}) => [task.id, text(task.title)]),
  );
  for (const item of rows) {
    body.addChild(treeRow(item, theme, model.owner, selected));
    const blockers = item.node.task.blocked_by;
    if (blockers.length)
      body.addChild(
        note(
          `Needs: ${blockers
            .slice(0, 8)
            .map(id => labels.get(id) ?? text(id))
            .join(
              ', ',
            )}${blockers.length > 8 ? ` · +${blockers.length - 8} more` : ''}`,
          theme,
          'muted',
          Math.min(item.depth, 6) * 2 + 13,
        ),
      );
  }
  const {hidden_closed} = snapshot.overview;
  if (!rows.length)
    body.addChild(
      note(
        hidden_closed ? 'No open slices remain.' : 'No slices recorded.',
        theme,
        'muted',
      ),
    );
  if (rows.length >= 100)
    body.addChild(
      note('Showing 100 rows · 3 record for the full-record command', theme),
    );
  if (tree.warning) body.addChild(note(tree.warning, theme, 'warning'));
  if (hidden_closed)
    body.addChild(
      note(
        `${hidden_closed} closed task${hidden_closed === 1 ? '' : 's'} hidden · h shows them`,
        theme,
      ),
    );
  const {root} = snapshot.overview.goal;
  body.addChild(
    heading(
      'Result',
      root.result_event !== null ? `event ${root.result_event}` : '',
      theme,
    ),
  );
  body.addChild(
    root.result
      ? new MarkdownView(root.result, theme)
      : note('No goal result recorded yet.', theme, 'dim', 2),
  );
  if (view.brief?.warning)
    body.addChild(note(text(view.brief.warning), theme, 'warning'));
  body.addChild(note('Recorded state · not live execution', theme));
  return {body, treeTop};
}

/** Non-modal goal overview. Pi owns focus; this owns sections, folds and history. */
export class GoalPanel extends Inspector implements Focusable {
  private model: PanelModel | undefined;
  private theme: Theme | undefined;
  private section: PanelSection = 'overview';
  private history = false;
  private readonly collapsed = new Set<string>();
  private selected: string | undefined;
  private hasFocus = false;
  private readonly actions: PanelActions;
  private readonly requestRender: () => void;

  constructor(
    bindings: Pick<KeybindingsManager, 'matches'>,
    actions: PanelActions,
    redraw: () => void,
    height: () => number,
  ) {
    super(bindings, actions.release, redraw, height);
    this.actions = actions;
    this.requestRender = redraw;
  }

  private rows(view: GoalView): TreeRow[] {
    return view.snapshot && !view.error
      ? treeRows(goalTree(view.snapshot), this.collapsed).slice(0, 100)
      : [];
  }

  override render(width: number): string[] {
    return width > 0 ? super.render(width) : [];
  }

  get focused(): boolean {
    return this.hasFocus;
  }
  set focused(value: boolean) {
    if (value === this.hasFocus) return;
    this.hasFocus = value;
    this.refresh();
  }

  /** The displayed snapshot: closed tasks join only on request or for a closed goal. */
  private shown(): GoalView | undefined {
    const view = this.model?.view;
    const snapshot = view?.snapshot;
    if (!view || !snapshot || view.error || !this.history) return view;
    if (!snapshot.allTasks && snapshot.overview.hidden_closed) return view;
    return {...view, snapshot: withHierarchy(snapshot, true), history: true};
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
    const snapshot = model.view.snapshot;
    if (snapshot) {
      const nodes = goalTree(snapshot).nodes;
      for (const id of this.collapsed)
        if (!nodes.has(id) || !isGroup(nodes.get(id)!))
          this.collapsed.delete(id);
    }
    this.refresh(changed);
  }

  private refresh(resetScroll = false, reveal = false): void {
    const model = this.model;
    const theme = this.theme;
    const view = this.shown();
    if (!model || !theme || !view) return;
    let body: Component;
    let groups: TreeRow[] = [];
    if (this.section === 'overview') {
      const rows = this.rows(view);
      groups = rows.filter(item => isGroup(item.node));
      if (!groups.some(item => item.node.task.id === this.selected))
        this.selected = groups[0]?.node.task.id;
      const built = overview(
        model,
        view,
        theme,
        rows,
        this.hasFocus ? this.selected : undefined,
      );
      body = built.body;
      if (reveal && this.selected) {
        const index = rows.findIndex(
          item => item.node.task.id === this.selected,
        );
        const blockers = rows
          .slice(0, index)
          .filter(item => item.node.task.blocked_by.length).length;
        this.revealLine(built.treeTop + index + blockers);
      }
    } else
      body =
        this.section === 'brief'
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
      view.snapshot?.allTasks && 'h closed',
      model.followed > 1 && 'Tab next goal',
      this.section === 'overview' && groups.length && '[/] group · ←/→ fold',
      '↑/↓ scroll · Esc editor',
      `${PANEL_KEY} close`,
    ];
    this.setContent(
      {
        title: view.snapshot?.overview.goal.root.title ?? view.focus.id,
        detail: view.error
          ? 'unavailable'
          : [
              view.snapshot?.overview.goal.root.status,
              this.section,
              this.history && 'with closed',
            ]
              .filter(Boolean)
              .join(' · '),
        body,
        hint: this.hasFocus
          ? keys.filter(Boolean).join(' · ')
          : `/goal to scroll and fold · ${PANEL_KEY} close`,
      },
      theme,
      resetScroll,
    );
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
      this.selected &&
      (matchesKey(data, '[') ||
        matchesKey(data, ']') ||
        matchesKey(data, 'space') ||
        matchesKey(data, 'left') ||
        matchesKey(data, 'right'))
    ) {
      const groups = this.rows(this.shown()!).filter(item =>
        isGroup(item.node),
      );
      if (matchesKey(data, '[') || matchesKey(data, ']')) {
        const current = Math.max(
          0,
          groups.findIndex(item => item.node.task.id === this.selected),
        );
        this.selected =
          groups[
            (current + (matchesKey(data, ']') ? 1 : groups.length - 1)) %
              groups.length
          ]!.node.task.id;
      } else if (
        matchesKey(data, 'left') ||
        (matchesKey(data, 'space') && !this.collapsed.has(this.selected))
      )
        this.collapsed.add(this.selected);
      else this.collapsed.delete(this.selected);
      this.refresh(false, true);
    } else {
      super.handleInput(data);
      return;
    }
    this.requestRender();
  }
}
