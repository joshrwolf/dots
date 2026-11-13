import {sliceByColumn, visibleWidth} from '@earendil-works/pi-tui';
import {labelText} from '@joshrwolf/pi-kit/ui';
import type {Brief, GoalSnapshot, GoalTask, Hold} from './source.ts';

/** A hold as dots states it: what the work waits on. */
export function holdText(hold: Hold): string {
  return `ancestor ${hold.task} is held`;
}

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
/** Progress counts only; see compactSummary. */
export function progressSummary(view: GoalView): string {
  const p = view.snapshot?.overview.progress;
  if (!p || view.error)
    return view.error ? 'unavailable · /goal refresh' : 'loading';
  return [
    p.total ? `${p.done}/${p.total} slices done` : 'No slices recorded',
    p.claimed && `${p.claimed} claimed`,
    p.held && `${p.held} held`,
    p.review && `${p.review} review`,
    p.dropped && `${p.dropped} dropped`,
  ]
    .filter(Boolean)
    .join(' · ');
}
/** The brief warning's leading phrase, such as "brief truncated"; the brief carries the rest. */
export const briefNote = (view: GoalView): string | undefined =>
  view.brief?.warning?.split(/[;:]/)[0]!.trim().toLowerCase();
export function compactSummary(view: GoalView): string {
  const note = !view.error && view.snapshot && briefNote(view);
  return [progressSummary(view), note].filter(Boolean).join(' · ');
}
export function summary(view: GoalView): string {
  if (view.error) return 'unavailable · /goal refresh';
  const p = view.snapshot?.overview.progress;
  if (!p) return 'loading';
  return `${p.done} done · ${p.open} open · ${p.held} held · ${p.dropped} dropped · ${p.ready} ready · ${p.review} review`;
}
export function artifacts(task: GoalTask, evidenceLimit: number): string[] {
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
export function latestEvent(task: GoalTask): string[] {
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
    ...(goal.root.foreign
      ? [
          `Read-only here: homed on ${goal.root.foreign.home}; take over with dots task share take ${goal.root.foreign.root}`,
        ]
      : []),
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
      task.holds.length > 0 && `held: ${task.holds.map(holdText).join('; ')}`,
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
      `${snapshot.overview.hidden_closed} closed task details hidden; use C in Rex Goals or dots task goal show ${goal.root.id} --all shows them.`,
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
