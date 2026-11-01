import {createHash} from 'node:crypto';
import {sliceByColumn} from '@earendil-works/pi-tui';
import {labelText, sourceText} from '@joshrwolf/pi-kit/ui';
import type {GoalView} from './present.ts';
import type {GoalTask} from './source.ts';

export const GOAL_SECTION_PREFIX = 'dots_goal_';
const TASK_CHARS = 8192;
const TASK_ROWS = 40;
// The goal's own result is its running handoff: where things stand and what is next.
const HANDOFF_CHARS = 2048;
const text = (value: string, max = 160) =>
  sliceByColumn(labelText(value), 0, max, true);

/** Source identity, not a render hash: revisions update the same native section. */
export function goalSectionKey(view: GoalView): string {
  const digest = createHash('sha256')
    .update(JSON.stringify([view.focus.project, view.focus.id]))
    .digest('hex');
  return `${GOAL_SECTION_PREFIX}${digest}`;
}

/** Independent of tasks, workers and clocks. The document owns the working agreement. */
export function briefContext(view: GoalView): string {
  const identity = `Goal ${view.focus.id} · project ${view.focus.project}`;
  if (view.error)
    return `${identity}\nBrief unavailable: ${text(view.error, 512)}. Refresh before acting.`;
  const goal = view.snapshot?.overview.goal;
  if (!goal) return `${identity}\nBrief not loaded. Refresh before acting.`;
  const brief = view.brief ?? goal.briefContent;
  const body = sourceText(brief.text);
  return [
    identity,
    'The brief records the agreed outcome and working agreement; the current user request takes precedence.',
    `Brief: ${goal.brief} · revision ${brief.revision ?? 'unavailable'}`,
    body,
    ...(brief.warning ? [brief.warning] : []),
    ...(brief.warning
      ? [
          `Read the full brief at ${goal.brief} before acting; this preview may omit working agreements.`,
        ]
      : []),
  ].join('\n');
}

/** Bounded current state: counts and unfinished tasks, attention first. */
export function taskContext(view: GoalView): string {
  const identity = `Goal ${view.focus.id} · project ${view.focus.project}`;
  if (view.error)
    return `${identity}\nTask state unavailable: ${text(view.error, 512)} · /goal refresh`;
  const snapshot = view.snapshot;
  if (!snapshot) return `${identity}\nTask state not loaded.`;
  const {goal, progress: p} = snapshot.overview;
  const command = `dots task --project ${view.focus.project}`;
  const counts = [
    [p.open, 'open'],
    [p.review, 'in review'],
    [p.held, 'held'],
    [p.done, 'done'],
    [p.dropped, 'dropped'],
  ] as const;
  const progress = counts
    .filter(([count]) => count > 0)
    .map(([count, label]) => `${count} ${label}`)
    .join(' · ');
  const lines = [
    `${identity} · ${text(goal.root.title)} [${goal.root.status}]${goal.root.owner ? ` · claimed by ${text(goal.root.owner, 64)}` : ''}${goal.root.result_event !== null ? ` · result event ${goal.root.result_event}` : ''}`,
    `Tasks: ${progress || 'none yet'}`,
  ];
  if (goal.root.result !== null) {
    const handoff = [...sourceText(goal.root.result)];
    lines.push(
      'Current state:',
      handoff.length > HANDOFF_CHARS
        ? `${handoff.slice(0, HANDOFF_CHARS).join('')}…\n[Truncated; ${command} show ${view.focus.id}]`
        : handoff.join(''),
    );
  }
  const tasks = snapshot.overview.tasks.filter(
    task => task.status !== 'done' && task.status !== 'dropped',
  );
  const rank = (task: GoalTask) =>
    task.status === 'held'
      ? 0
      : task.status === 'review'
        ? 1
        : task.owner
          ? 2
          : task.blocked_by.length
            ? 3
            : 4;
  tasks.sort((a, b) => rank(a) - rank(b) || a.id.localeCompare(b.id));
  let remaining = TASK_CHARS;
  let shown = 0;
  for (const task of tasks.slice(0, TASK_ROWS)) {
    const fields = [
      task.parent && task.parent !== goal.root.id && `parent ${task.parent}`,
      task.children > 0 && `group of ${task.children}`,
      task.owner && `claimed by ${text(task.owner, 64)}`,
      task.branch && `branch ${text(task.branch, 80)}`,
      task.blocked_by.length > 0 &&
        `blocked by ${task.blocked_by.slice(0, 4).join(', ')}${task.blocked_by.length > 4 ? '…' : ''}`,
      // The event number changes when a result is revised, so a reader knows to reread it.
      task.result_event !== null && `result event ${task.result_event}`,
    ].filter(Boolean);
    const line = `${task.id} [${task.status}] ${text(task.title, 120)}${fields.length ? ` — ${fields.join('; ')}` : ''}`;
    if (line.length > remaining) break;
    lines.push(line);
    remaining -= line.length + 1;
    shown++;
  }
  if (tasks.length > shown)
    lines.push(`${tasks.length - shown} more unfinished tasks not shown.`);
  if (!tasks.length) lines.push('No unfinished tasks.');
  lines.push(
    `Results and history: ${command} show TASK_ID; every task: ${command} goal show ${view.focus.id} --all`,
  );
  return lines.join('\n');
}

/** Also used by goal_focus: run-start hooks have not run again after that tool call. */
export function orientation(view: GoalView): string {
  return `${briefContext(view)}\n\n${taskContext(view)}`;
}

export function goalSections(view: GoalView): Record<string, string> {
  const key = goalSectionKey(view);
  return {
    [`${key}_brief`]: briefContext(view),
    [`${key}_tasks`]: taskContext(view),
  };
}
