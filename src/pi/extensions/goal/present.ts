import type {Theme} from '@earendil-works/pi-coding-agent';
import {
  matchesKey,
  sliceByColumn,
  visibleWidth,
  VStack,
  type Component,
} from '@earendil-works/pi-tui';
import {PriorityRow, labelText} from '@joshrwolf/pi-kit/ui';
import {MarkdownView} from '../../src/ui/markdown.ts';
import {Inspector} from '../../src/ui/inspector.ts';
import {OutputView} from '../../src/ui/card.ts';
import type {Brief, GoalSnapshot, GoalTask} from './source.ts';
import {compactSummary, goalOverview} from './dashboard.ts';
import {goalTree, isGroup, treeRows, type TreeState} from './hierarchy.ts';
export {compactSummary, GoalWidget, FollowedWidget} from './dashboard.ts';

export interface Focus {
  readonly id: string;
  readonly project: string;
}
export interface GoalView {
  readonly focus: Focus;
  readonly snapshot?: GoalSnapshot;
  readonly brief?: Brief;
  readonly history?: boolean;
  readonly error?: string;
}
/** Bounded in display columns at grapheme boundaries; never splits a surrogate pair or cluster. */
const text = (value: string, max = 512) =>
  sliceByColumn(labelText(value), 0, max, true);
const exceeds = (value: string, max: number) =>
  visibleWidth(labelText(value)) > max;
export function summary(view: GoalView): string {
  if (view.error) return 'unavailable · /goal refresh';
  const p = view.snapshot?.overview.progress;
  if (!p) return 'loading';
  return `${p.done} done · ${p.open} open · ${p.held} held · ${p.dropped} dropped · ${p.ready} ready · ${p.review} review`;
}
function artifacts(task: GoalTask, evidenceLimit: number): string[] {
  const lines = task.references
    .slice(0, 8)
    .map(
      ref =>
        `Reference: ${ref.label ? `${text(ref.label, 256)} ` : ''}${text(ref.url, 4096)}`,
    );
  if (task.references.length > 8)
    lines.push(
      `[${task.references.length - 8} references omitted; dots task show ${task.id} --json]`,
    );
  if (task.result) {
    lines.push(`Result: ${text(task.result, 2048)}`);
    if (exceeds(task.result, 2048))
      lines.push(`[Result truncated; dots task show ${task.id} --json]`);
  }
  if (task.result_event !== null)
    lines.push(
      `Result event: ${task.result_event} · /goal evidence ${task.id} ${task.result_event}`,
    );
  if (task.evidence.length > evidenceLimit)
    lines.push(
      `${task.evidence.length} pinned evidence selection(s); ${task.purged_evidence.length} purged. ${task.evidence.length - evidenceLimit} omitted; /goal evidence ${task.id} ${task.result_event ?? 'current'}`,
    );
  for (const evidence of task.evidence.slice(0, evidenceLimit)) {
    const purged = task.purged_evidence.some(
      item => item.slug === evidence.slug && item.digest === evidence.digest,
    );
    lines.push(
      `Evidence: ${evidence.slug} ${evidence.digest} · ${purged ? 'purged; unavailable' : 'pinned; bytes not revalidated'}`,
    );
  }
  return lines;
}
const EVENT_DETAIL = 512;
/** The latest recorded event only; never a reconstructed history or handoff. */
function latestEvent(task: GoalTask): string[] {
  const event = task.last_event;
  if (!event || (event.kind === 'created' && !event.detail)) return [];
  // Result-recording events repeat the result shown above; keep only when, what, and who.
  const detail = event.detail === task.result ? null : event.detail;
  const fields = [
    new Date(event.at * 1000).toISOString(),
    text(event.kind, 64),
    event.actor && `by ${text(event.actor, 128)}`,
    detail && text(detail, EVENT_DETAIL),
  ].filter(Boolean);
  const lines = [`  Latest event (historical): ${fields.join(' · ')}`];
  if (detail && exceeds(detail, EVENT_DETAIL))
    lines.push(`  [Event detail truncated; dots task show ${task.id} --json]`);
  return lines;
}
function workerObservations(snapshot: GoalSnapshot): string[] {
  const observed = snapshot.workers;
  if (!observed) return ['Worker observations not loaded.'];
  if (observed.warning) return [text(observed.warning)];
  const lines = [
    'Contributing workers (recorded observations; not task acceptance):',
  ];
  for (const worker of observed.rows) {
    const stale =
      !worker.ownerAvailable &&
      (worker.state === 'running' || worker.state === 'asking');
    lines.push(
      `Worker ${text(worker.id)} · ${text(worker.label)} · task ${worker.task_id} · recorded ${text(worker.state)} at ${new Date(worker.updated_at).toISOString()}${stale ? ' · stale: owner process unavailable' : ''}`,
      `  Recorded checkout: ${text(worker.cwd, 1024)}`,
      `  Parent worker: ${worker.parent ? text(worker.parent) : 'none (spawned by owner session)'} · owner session: ${text(worker.owner_session ?? 'unavailable', 1024)}`,
    );
    if (worker.session_file)
      lines.push(`  Worker session: ${text(worker.session_file, 1024)}`);
  }
  if (!observed.rows.length)
    lines.push(
      'No associated workers recorded; this is not proof that no execution exists.',
    );
  if (observed.truncated)
    lines.push(
      '[Worker observations limited to 20 newest records; inspect owning sessions for more.]',
    );
  lines.push(
    'Check worker state in its owning session. Use native wt list/Git for current checkout facts; paths above do not establish current branches, cleanliness, or ownership.',
  );
  return lines;
}
export function inspection(
  view: GoalView,
  taskLimit = 100,
  evidenceLimit = 32,
): string {
  const lines = [
    `Goal ${view.focus.id}`,
    `Project: ${view.focus.project}`,
    'Focus only; no automatic execution.',
  ];
  if (view.error)
    return [
      ...lines,
      `Unavailable: ${view.error}`,
      'Refresh before relying on previous goal context.',
    ].join('\n');
  const snapshot = view.snapshot;
  if (!snapshot) return [...lines, 'Loading…'].join('\n');
  const {goal, tasks} = snapshot.overview;
  lines.push(
    goal.root.title,
    `Root status: ${goal.root.status}`,
    `Recorded at: ${new Date(snapshot.now * 1000).toISOString()}`,
    summary(view),
    'Counts cover leaf tasks. Claimed includes review work. Done records accepted completion, not independently verified evidence.',
    'Git/provider state is not refreshed here; worker observations below are recorded, not execution authority.',
    `Brief: ${goal.brief}`,
    ...(goal.briefContent.revision
      ? [`Brief revision: ${goal.briefContent.revision}`]
      : []),
    ...artifacts(goal.root, evidenceLimit),
    '',
    'Slices:',
  );
  let eventShown = false;
  for (const task of tasks.slice(0, taskLimit)) {
    const details = [
      task.parent && `parent ${task.parent}`,
      task.children > 0 && `group (${task.children} children)`,
      task.owner && `claimed by ${text(task.owner, 128)}`,
      task.blocked_by.length > 0 && `blocked by ${task.blocked_by.join(', ')}`,
      task.branch && `branch ${text(task.branch)}`,
    ].filter(Boolean);
    const events = latestEvent(task);
    if (events.length) eventShown = true;
    lines.push(
      `${task.id} [${task.status}] ${text(task.title)}${details.length ? ` — ${details.join('; ')}` : ''}`,
      ...artifacts(task, evidenceLimit),
      ...events,
    );
  }
  if (eventShown)
    lines.push(
      "Latest events show only each slice's most recent record, not a handoff or acceptance; a later claim or update can supersede an earlier note. dots task show ID shows the result and recent events; for older events, page dots task events --json --under ID from --since 0.",
    );
  if (!tasks.length)
    lines.push(
      snapshot.overview.hidden_closed > 0
        ? 'No open slices remain.'
        : 'No slices recorded.',
    );
  if (snapshot.overview.hidden_closed > 0)
    lines.push(
      `${snapshot.overview.hidden_closed} closed task details hidden; /goal history or dots task goal show ${goal.root.id} --all shows history.`,
    );
  if (tasks.length > taskLimit)
    lines.push(
      `[${tasks.length - taskLimit} tasks omitted; use dots task goal show ${goal.root.id}${view.history ? ' --all' : ''} --json for all.]`,
    );
  lines.push(
    '',
    ...workerObservations(snapshot),
    '',
    'Brief content:',
    view.brief?.text ?? 'Brief not loaded.',
  );
  if (view.brief?.warning) lines.push('', view.brief.warning);
  return lines.join('\n');
}

type Section = 'slices' | 'brief' | 'details';

export function overview(model: GoalView): string {
  if (model.error || !model.snapshot) return inspection(model);
  const {goal, tasks, hidden_closed} = model.snapshot.overview;
  const labels = new Map(tasks.map(task => [task.id, text(task.title)]));
  const lines = [
    compactSummary(model),
    ...(goal.root.status !== 'open' ? [`Goal: ${goal.root.status}`] : []),
    '',
  ];
  for (const task of tasks.slice(0, 100)) {
    const claimed =
      (task.status === 'open' || task.status === 'review') && task.owner;
    const state =
      task.children > 0
        ? `Group (${task.status})`
        : task.status !== 'open'
          ? task.status === 'held'
            ? 'Held'
            : task.status === 'review'
              ? 'Review'
              : task.status === 'done'
                ? 'Done'
                : 'Dropped'
          : claimed
            ? 'Claimed'
            : task.blocked_by.length
              ? 'Waiting'
              : 'Open';
    lines.push(`${state.padEnd(9)} ${text(task.title)}`);
    if (task.blocked_by.length)
      lines.push(
        `          Needs: ${task.blocked_by.map(id => labels.get(id) ?? id).join(', ')}`,
      );
  }
  if (!tasks.length)
    lines.push(
      hidden_closed ? 'No open slices remain.' : 'No slices recorded.',
    );
  if (tasks.length > 100)
    lines.push(
      `${tasks.length - 100} more tasks; see 3 details for the full-record command.`,
    );
  if (hidden_closed)
    lines.push(
      '',
      `${hidden_closed} closed task details hidden · close and use /goal history`,
    );
  if (model.brief?.warning) lines.push('', model.brief.warning);
  return lines.join('\n');
}

export function setInspection(
  view: Inspector,
  model: GoalView,
  theme: Theme,
  section: Section = 'slices',
  resetScroll = false,
  treeState?: TreeState,
): void {
  let body: Component;
  if (section === 'slices') body = goalOverview(model, theme, treeState);
  else {
    const heading = new PriorityRow();
    heading.update(
      {
        start: [
          {
            text: section === 'brief' ? 'Brief' : 'Record details',
            tone: 'accent',
            bold: true,
            priority: 100,
            shrink: 'clip',
          },
        ],
        end: [],
      },
      theme,
    );
    body = new VStack([
      heading,
      section === 'brief'
        ? new MarkdownView(
            model.error ??
              [model.brief?.text ?? 'Brief not loaded.', model.brief?.warning]
                .filter(Boolean)
                .join('\n\n'),
            theme,
          )
        : new OutputView(inspection(model)),
    ]);
  }
  view.setContent(
    {
      title: model.snapshot?.overview.goal.root.title ?? 'Goal inspection',
      detail: model.error
        ? 'unavailable'
        : [
            model.snapshot?.overview.goal.root.status,
            model.history ? 'history' : section,
          ]
            .filter(Boolean)
            .join(' · '),
      body,
      hint:
        section === 'slices' && treeState
          ? '1 slices · 2 brief · 3 details · [/] group · ←/→ fold · ↑/↓ scroll · Esc close'
          : '1 slices · 2 brief · 3 details · ↑/↓ scroll · Enter/Esc close',
    },
    theme,
    resetScroll,
  );
}

export class GoalInspector extends Inspector {
  private goal: {model: GoalView; theme: Theme; redraw: () => void} | undefined;
  private section: Section = 'slices';
  private readonly collapsed = new Set<string>();
  private selected: string | undefined;

  private refresh(resetScroll = false, reveal = false): void {
    if (!this.goal) return;
    const {model, theme} = this.goal;
    const rows =
      model.snapshot && !model.error
        ? treeRows(goalTree(model.snapshot), this.collapsed).slice(0, 100)
        : [];
    const groups = rows.filter(row => isGroup(row.node));
    if (!groups.some(row => row.node.task.id === this.selected))
      this.selected = groups[0]?.node.task.id;
    setInspection(
      this,
      model,
      theme,
      this.section,
      resetScroll,
      groups.length
        ? {collapsed: this.collapsed, selected: this.selected}
        : undefined,
    );
    if (reveal && this.section === 'slices' && this.selected) {
      const index = rows.findIndex(row => row.node.task.id === this.selected);
      const dependencies = rows
        .slice(0, index)
        .filter(row => row.node.task.blocked_by.length).length;
      this.revealLine(
        3 +
          Number(model.snapshot?.overview.goal.root.status !== 'open') +
          index +
          dependencies,
      );
    }
  }

  setGoal(model: GoalView, theme: Theme, redraw: () => void): void {
    const changed = this.goal?.model.focus.id !== model.focus.id;
    if (changed) {
      this.section = 'slices';
      this.collapsed.clear();
      this.selected = undefined;
    }
    this.goal = {model, theme, redraw};
    if (model.snapshot) {
      const nodes = goalTree(model.snapshot).nodes;
      for (const id of this.collapsed)
        if (!nodes.has(id) || !isGroup(nodes.get(id)!))
          this.collapsed.delete(id);
    }
    this.refresh(changed);
  }
  override handleInput(data: string): void {
    const section = matchesKey(data, '1')
      ? 'slices'
      : matchesKey(data, '2')
        ? 'brief'
        : matchesKey(data, '3')
          ? 'details'
          : undefined;
    if (section && this.goal) {
      this.section = section;
      this.refresh(true);
      this.goal.redraw();
    } else if (
      this.section === 'slices' &&
      this.goal?.model.snapshot &&
      !this.goal.model.error &&
      (matchesKey(data, '[') ||
        matchesKey(data, ']') ||
        matchesKey(data, 'space') ||
        matchesKey(data, 'left') ||
        matchesKey(data, 'right'))
    ) {
      const groups = treeRows(
        goalTree(this.goal.model.snapshot),
        this.collapsed,
      )
        .slice(0, 100)
        .filter(row => isGroup(row.node));
      if (!groups.length) return;
      if (matchesKey(data, '[') || matchesKey(data, ']')) {
        const current = Math.max(
          0,
          groups.findIndex(row => row.node.task.id === this.selected),
        );
        this.selected =
          groups[
            (current + (matchesKey(data, ']') ? 1 : groups.length - 1)) %
              groups.length
          ]!.node.task.id;
      } else if (this.selected) {
        if (
          matchesKey(data, 'left') ||
          (matchesKey(data, 'space') && !this.collapsed.has(this.selected))
        )
          this.collapsed.add(this.selected);
        else this.collapsed.delete(this.selected);
      }
      this.refresh(false, true);
      this.goal.redraw();
    } else super.handleInput(data);
  }
}
