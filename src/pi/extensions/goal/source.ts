import path from 'node:path';
import {isTimestamp} from '../../src/date.ts';
import {DotsError, dots} from '@joshrwolf/pi-kit/dots';
import {agentEnv, type Agent} from '../../src/host/agent.ts';
import {readTaskWorkers, type TaskWorkers} from '../workers/registry.ts';

export interface Evidence {
  readonly slug: string;
  readonly digest: string;
}
export interface GoalTask {
  readonly id: string;
  readonly title: string;
  readonly status: 'open' | 'review' | 'held' | 'done' | 'dropped';
  readonly parent: string | null;
  readonly children: number;
  /** Claiming runtime actor; a claim lasts until released, handed off, or closed. */
  readonly owner: string | null;
  readonly result: string | null;
  /** Event that recorded the current result. */
  readonly result_event: number | null;
  readonly evidence: readonly Evidence[];
  readonly purged_evidence: readonly Evidence[];
  readonly references: readonly ExternalReference[];
  readonly branch: string | null;
  readonly blocked_by: readonly string[];
  /** The goal root this task belongs to; itself for a root. */
  readonly goal: string | null;
  /** Why work waits beyond its prerequisites, as dots derives it. */
  readonly holds: readonly Hold[];
  /** Latest recorded event only; a later claim or update can supersede a useful note. */
  readonly last_event: TaskEvent | null;
  /** Set when another machine homes this task's tree; it is read-only here. */
  readonly foreign: Foreign | null;
}
/** A tree imported from the machine that alone changes it. */
export interface Foreign {
  readonly root: string;
  readonly home: string;
  /** Unix seconds when the home wrote the imported version. */
  readonly written_at: number | null;
  /** Unix seconds when this machine imported it. */
  readonly imported_at: number | null;
}
/** An ancestor is held; its whole subtree waits. */
export interface Hold {
  readonly kind: 'held_ancestor';
  readonly task: string;
}
export interface TaskEvent {
  readonly seq: number;
  /** Unix seconds. */
  readonly at: number;
  readonly kind: string;
  readonly actor: string | null;
  readonly detail: string | null;
  readonly task: string;
}
export interface ExternalReference {
  readonly url: string;
  readonly label: string | null;
}
export interface Goal {
  readonly root: GoalTask;
  readonly brief: string;
  readonly briefContent: Brief;
}
export interface Progress {
  readonly total: number;
  readonly open: number;
  readonly held: number;
  readonly done: number;
  readonly dropped: number;
  readonly ready: number;
  readonly claimed: number;
  readonly review: number;
}
export interface GoalSnapshot {
  readonly project: string;
  readonly now: number;
  readonly overview: {
    readonly goal: Goal;
    readonly tasks: readonly GoalTask[];
    readonly hidden_closed: number;
    readonly progress: Progress;
    /** dots' drawing of every leaf in `tasks`. */
    readonly graph?: DependencyGraph;
    /** The same over unclosed leaves, when `tasks` includes a closed leaf. */
    readonly open_graph?: DependencyGraph | null;
  };
  /** Complete descendants from the same --all snapshot, for hierarchy rollups only. */
  readonly allTasks?: readonly GoalTask[];
  readonly workers?: TaskWorkers;
}
/** Leaf dependencies as dots draws them; Pi only displays the lines. */
export interface DependencyGraph {
  /** Prerequisites above dependents: a node line, then link lines, one width. */
  readonly rows: readonly {
    readonly task: string;
    readonly lines: readonly string[];
  }[];
  readonly notes: readonly string[];
}
export interface GoalList {
  readonly project: string;
  readonly goals: readonly Goal[];
}
export interface Brief {
  readonly text: string;
  readonly revision?: string;
  readonly warning?: string;
}
/** One goal, exactly: its registered project key and root task. */
export interface GoalRef {
  readonly project: string;
  readonly id: string;
}
export interface GoalSource {
  list(cwd: string, signal: AbortSignal, claimedBy?: string): Promise<GoalList>;
  /** An explicit project key selects its task list instead of the cwd's project. */
  show(
    cwd: string,
    id: string,
    signal: AbortSignal,
    history?: boolean,
    project?: string,
  ): Promise<GoalSnapshot>;
  /** Release every claim `agent` holds, in every project, acting as it. */
  release(agent: Agent): Promise<void>;
  /**
   * The goals whose open or in-review root `agent` claims, in every
   * registered project: the store's root claims, read fresh each time.
   */
  coordinating(agent: Agent, signal: AbortSignal): Promise<readonly GoalRef[]>;
}

// Decode the fields this consumer uses; ignore additional CLI fields. Invalid
// data is an unavailable observation, never an empty/successful goal.
function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Invalid dots goal response');
  return value as Record<string, unknown>;
}
function string(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Invalid dots goal text');
  return value;
}
function count(value: unknown): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0)
    throw new Error('Invalid dots goal count');
  return value;
}
function timestamp(value: unknown): number {
  if (!isTimestamp(value, 'seconds'))
    throw new Error('Invalid dots goal timestamp (Unix seconds)');
  return value;
}
function nullableString(value: unknown): string | null {
  return value === null ? null : string(value);
}
function array(value: unknown): unknown[] {
  if (!Array.isArray(value)) throw new Error('Invalid dots goal list');
  return value;
}
function reference(value: unknown): ExternalReference {
  const v = record(value);
  return {url: string(v.url), label: nullableString(v.label)};
}
function event(value: unknown): TaskEvent | null {
  if (value === undefined || value === null) return null;
  const v = record(value);
  return {
    seq: count(v.seq),
    at: timestamp(v.at),
    kind: string(v.kind),
    actor: nullableString(v.actor),
    detail: nullableString(v.detail),
    task: string(v.task),
  };
}
export function decodeEvidence(value: unknown): Evidence {
  const v = record(value);
  const slug = string(v.slug);
  const digest = string(v.digest);
  if (
    !/^[a-z0-9](?:[a-z0-9-]{0,98}[a-z0-9])?$/.test(slug) ||
    !/^sha256:[a-f0-9]{64}$/.test(digest)
  )
    throw new Error('Invalid dots evidence identity');
  return {slug, digest};
}
function evidenceList(value: unknown): Evidence[] {
  const selections = array(value);
  if (selections.length > 32) throw new Error('Invalid dots evidence limit');
  return selections.map(decodeEvidence);
}
function token(value: unknown): number | null {
  if (value === null) return null;
  const result = count(value);
  if (!result) throw new Error('Invalid dots result event');
  return result;
}
function hold(value: unknown): Hold {
  const v = record(value);
  if (v.kind === 'held_ancestor') return {kind: v.kind, task: string(v.task)};
  throw new Error('Invalid dots task hold');
}
function foreign(value: unknown): Foreign | null {
  if (value === null) return null;
  const v = record(value);
  const time = (field: unknown) => (field === null ? null : timestamp(field));
  return {
    root: string(v.root),
    home: string(v.home),
    written_at: time(v.written_at),
    imported_at: time(v.imported_at),
  };
}
function task(value: unknown): GoalTask {
  const v = record(value);
  if (!['open', 'review', 'held', 'done', 'dropped'].includes(string(v.status)))
    throw new Error('Invalid dots task status');
  return {
    id: string(v.id),
    title: string(v.title),
    status: v.status as GoalTask['status'],
    parent: nullableString(v.parent),
    children: count(v.children),
    owner: nullableString(v.owner),
    result: nullableString(v.result),
    result_event: token(v.result_event),
    evidence: evidenceList(v.evidence),
    purged_evidence: evidenceList(v.purged_evidence),
    references: array(v.references).map(reference),
    branch: nullableString(v.branch),
    blocked_by: array(v.blocked_by).map(string),
    goal: nullableString(v.goal),
    holds: array(v.holds).map(hold),
    last_event: event(v.last_event),
    foreign: foreign(v.foreign),
  };
}
function goal(value: unknown): Goal {
  const v = record(value);
  const root = task(v.root);
  const reference = record(v.brief);
  const brief = string(reference.path);
  if (root.parent !== null || !path.isAbsolute(brief))
    throw new Error('Invalid dots goal root or brief reference');
  const diagnostic = nullableString(reference.diagnostic);
  let briefContent: Brief;
  if (reference.document === null) {
    briefContent = {
      text: '',
      warning: diagnostic
        ? `Brief unavailable: ${diagnostic}`
        : 'Brief not loaded; inspect the goal.',
    };
  } else {
    const document = record(reference.document);
    if (typeof document.exists !== 'boolean' || string(document.path) !== brief)
      throw new Error('Invalid dots brief document');
    const revision = string(document.revision);
    if (
      !(document.exists
        ? /^sha256:[a-f0-9]{64}$/.test(revision)
        : revision === 'missing')
    )
      throw new Error('Invalid dots brief revision');
    const text = string(document.content);
    if (!document.exists && text !== '')
      throw new Error('Missing brief contains text');
    const nextLine =
      document.next_line === null ? null : count(document.next_line);
    if (count(document.start) !== 1 || (nextLine !== null && nextLine < 2))
      throw new Error('Invalid dots brief page');
    count(document.total_lines);
    briefContent = {
      text,
      revision,
      ...(diagnostic
        ? {warning: `Brief unavailable: ${diagnostic}`}
        : !document.exists
          ? {warning: 'Brief unavailable: missing'}
          : nextLine !== null
            ? {
                warning: `Brief truncated; read ${brief} from line ${nextLine} for the rest.`,
              }
            : {}),
    };
  }
  return {root, brief, briefContent};
}
export function decodeList(value: unknown): GoalList {
  const v = record(value);
  return {project: string(v.project), goals: array(v.goals).map(goal)};
}
function dependencyGraph(
  value: unknown,
  tasks: ReadonlySet<string>,
): DependencyGraph {
  const v = record(value);
  return {
    rows: array(v.rows).map(row => {
      const r = record(row);
      const task = string(r.task);
      const lines = array(r.lines).map(string);
      if (!tasks.has(task) || !lines.length)
        throw new Error('Invalid dots goal graph row');
      return {task, lines};
    }),
    notes: array(v.notes).map(string),
  };
}
export function decodeSnapshot(value: unknown): GoalSnapshot {
  const v = record(value);
  const overview = record(v.overview);
  const tasks = array(overview.tasks).map(task);
  const ids = new Set(tasks.map(task => task.id));
  // Progress belongs to the goal; the snapshot keeps it beside the tasks it counts.
  const p = record(record(overview.goal).progress);
  return {
    project: string(v.project),
    now: timestamp(v.now),
    overview: {
      goal: goal(overview.goal),
      tasks,
      hidden_closed: count(overview.hidden_closed),
      progress: {
        total: count(p.total),
        open: count(p.open),
        held: count(p.held),
        done: count(p.done),
        dropped: count(p.dropped),
        ready: count(p.ready),
        claimed: count(p.claimed),
        review: count(p.review),
      },
      graph: dependencyGraph(overview.graph, ids),
      open_graph:
        overview.open_graph === null
          ? null
          : dependencyGraph(overview.open_graph, ids),
    },
  };
}

async function query(
  cwd: string,
  args: string[],
  signal: AbortSignal,
): Promise<unknown> {
  try {
    return await dots(['task', 'goal', ...args], {directory: cwd, cwd, signal});
  } catch (error) {
    if (!(error instanceof DotsError)) throw error;
    if (error.reason === 'invalid-json')
      throw new Error('dots returned invalid goal JSON', {cause: error});
    throw new Error(
      `dots goal query failed: ${error.message}. Ensure the installed dots supports 'task goal'.`,
      {cause: error},
    );
  }
}

/** Keep ordinary context quiet while retaining complete records for group totals. */
export function withHierarchy(
  snapshot: GoalSnapshot,
  history = false,
): GoalSnapshot {
  if (snapshot.overview.hidden_closed !== 0 && !snapshot.allTasks)
    return snapshot;
  const allTasks = snapshot.allTasks ?? snapshot.overview.tasks;
  const tasks = history
    ? allTasks
    : allTasks.filter(
        task => task.status !== 'done' && task.status !== 'dropped',
      );
  return {
    ...snapshot,
    allTasks,
    overview: {
      ...snapshot.overview,
      tasks,
      hidden_closed: allTasks.length - tasks.length,
    },
  };
}

export interface EvidenceRecord {
  readonly project: string;
  readonly task: string;
  readonly seq: number | null;
  readonly current: number | null;
  readonly result: string | null;
  readonly evidence: readonly Evidence[];
  readonly purged: readonly Evidence[];
}
export interface OutputPage {
  readonly path: string;
  readonly state:
    | 'available'
    | 'missing'
    | 'corrupt'
    | 'unreadable'
    | 'unrenderable'
    | 'purged';
  readonly text: string;
  readonly next: number | null;
}
export interface OutputManifest {
  readonly revisions: readonly (Evidence & {
    readonly description: string;
    readonly provenance: string;
    readonly purged: boolean;
  })[];
  readonly next: number | null;
}
export interface EvidenceSource {
  attached(
    cwd: string,
    project: string,
    task: string,
    offset: number,
    signal: AbortSignal,
  ): Promise<{slugs: readonly string[]; next: number | null}>;
  manifest(
    cwd: string,
    project: string,
    slug: string,
    offset: number,
    signal: AbortSignal,
  ): Promise<OutputManifest>;
  record(
    cwd: string,
    id: string,
    signal: AbortSignal,
    seq?: number,
    project?: string,
  ): Promise<EvidenceRecord>;
  page(
    cwd: string,
    project: string,
    evidence: Evidence,
    start: number,
    signal: AbortSignal,
  ): Promise<OutputPage>;
}
function scoped(value: unknown, project?: string): Record<string, unknown> {
  const result = record(value);
  if (project !== undefined && string(result.project) !== project)
    throw new Error('Dots evidence project changed');
  return result;
}
export function decodeOutputPage(
  value: unknown,
  project: string,
  selection: Evidence,
  start: number,
): OutputPage {
  const output = record(scoped(value, project).output);
  const revision = decodeEvidence(output.revision);
  if (revision.slug !== selection.slug || revision.digest !== selection.digest)
    throw new Error('Dots output revision changed');
  const outputPath = string(output.path);
  if (!path.isAbsolute(outputPath)) throw new Error('Invalid dots output path');
  const content = record(output.content);
  const state = string(content.state);
  if (state === 'available') {
    const document = record(content.document);
    const next = document.next_line === null ? null : count(document.next_line);
    const total = count(document.total_lines);
    const text = string(document.content);
    if (
      document.exists !== true ||
      document.revision !== selection.digest ||
      document.path !== outputPath ||
      count(document.start) !== start ||
      Buffer.byteLength(text) > 16384 ||
      (next !== null && (next <= start || next > total))
    )
      throw new Error('Invalid dots output page');
    return {path: outputPath, state, text, next};
  }
  if (state === 'unreadable' || state === 'unrenderable')
    return {
      path: outputPath,
      state,
      text: string(content.diagnostic),
      next: null,
    };
  if (state === 'missing' || state === 'corrupt' || state === 'purged')
    return {path: outputPath, state, text: '', next: null};
  throw new Error('Invalid dots output state');
}
function nextOffset(value: unknown, offset: number): number | null {
  const next = value === null ? null : count(value);
  if (next !== null && next <= offset)
    throw new Error('Invalid dots manifest cursor');
  return next;
}
export const dotsEvidenceSource: EvidenceSource = {
  async attached(cwd, project, task, offset, signal) {
    const response = scoped(
      await dots(
        [
          'output',
          'list',
          '--task',
          task,
          '--offset',
          String(offset),
          '--project',
          project,
        ],
        {directory: cwd, cwd, signal},
      ),
      project,
    );
    const slugs = array(response.slugs).map(string);
    if (
      slugs.length > 50 ||
      slugs.some(slug => !/^[a-z0-9](?:[a-z0-9-]{0,98}[a-z0-9])?$/.test(slug))
    )
      throw new Error('Invalid dots output list');
    return {slugs, next: nextOffset(response.next_offset, offset)};
  },
  async manifest(cwd, project, slug, offset, signal) {
    const response = scoped(
      await dots(
        [
          'output',
          'revisions',
          slug,
          '--offset',
          String(offset),
          '--project',
          project,
        ],
        {directory: cwd, cwd, signal},
      ),
      project,
    );
    const output = record(response.output);
    if (output.slug !== slug) throw new Error('Dots output manifest changed');
    const revisions = array(output.revisions).map(value => {
      const revision = record(value);
      const identity = decodeEvidence(value);
      if (identity.slug !== slug || typeof revision.purged !== 'boolean')
        throw new Error('Invalid dots output manifest');
      return {
        ...identity,
        description: string(revision.description),
        provenance: string(revision.provenance),
        purged: revision.purged,
      };
    });
    if (revisions.length > 20) throw new Error('Invalid dots manifest limit');
    return {revisions, next: nextOffset(output.next_offset, offset)};
  },
  async record(cwd, id, signal, seq, project) {
    const options = {directory: cwd, cwd, signal};
    const shown = scoped(
      await dots(
        ['task', 'show', id, ...(project ? ['--project', project] : [])],
        options,
      ),
      project,
    );
    const currentTask = task(shown.task);
    if (currentTask.id !== id) throw new Error('Dots evidence task changed');
    const resolved = string(shown.project);
    const selected = seq ?? currentTask.result_event;
    if (selected === null)
      return {
        project: resolved,
        task: id,
        seq: null,
        current: null,
        result: currentTask.result,
        evidence: [],
        purged: [],
      };
    const response = scoped(
      await dots(
        [
          'task',
          'result-event',
          id,
          '--event',
          String(selected),
          '--project',
          resolved,
        ],
        options,
      ),
      resolved,
    );
    const event = record(response.event);
    if (token(event.seq) !== selected)
      throw new Error('Dots result event changed');
    return {
      project: resolved,
      task: id,
      seq: selected,
      current: currentTask.result_event,
      result: string(event.result),
      evidence: evidenceList(event.evidence),
      purged: evidenceList(event.purged_evidence),
    };
  },
  async page(cwd, project, evidence, start, signal) {
    return decodeOutputPage(
      await dots(
        [
          'output',
          'show',
          evidence.slug,
          '--digest',
          evidence.digest,
          '--start',
          String(start),
          '--lines',
          '200',
          '--project',
          project,
        ],
        {directory: cwd, cwd, signal},
      ),
      project,
      evidence,
      start,
    );
  },
};

export const dotsSource: GoalSource = {
  async list(cwd, signal, claimedBy) {
    return decodeList(
      await query(
        cwd,
        ['list', ...(claimedBy ? ['--claimed-by', claimedBy] : [])],
        signal,
      ),
    );
  },
  async show(cwd, id, signal, history = false, project) {
    const snapshot = decodeSnapshot(
      await query(
        cwd,
        ['show', id, '--all', ...(project ? ['--project', project] : [])],
        signal,
      ),
    );
    signal.throwIfAborted();
    return withHierarchy(
      {
        ...snapshot,
        workers: readTaskWorkers(snapshot.project, [
          id,
          ...snapshot.overview.tasks.map(task => task.id),
        ]),
      },
      history,
    );
  },
  async release(agent) {
    await dots(['task', '--owner', agent.owner, 'release', '--all'], {
      env: agentEnv(agent),
    });
  },
  async coordinating(agent, signal) {
    const v = record(
      await dots(['task', '--owner', agent.owner, 'goal', 'coordinating'], {
        signal,
        env: agentEnv(agent),
      }),
    );
    return array(v.goals).map(item => {
      const goal = record(item);
      const project = string(goal.project);
      const id = string(goal.goal);
      if (!/^t-[a-f0-9]+$/.test(id))
        throw new Error('Invalid dots coordinating goal');
      return {project, id};
    });
  },
};

/**
 * The substrate's changes marker when this machine shares task trees, so
 * imports from other machines can refresh the goals in view; otherwise
 * undefined, and goals refresh on runs and explicit reads only.
 */
export async function shareChanges(
  cwd: string,
  signal: AbortSignal,
): Promise<string | undefined> {
  const v = record(
    await dots(['task', 'share', 'status'], {directory: cwd, cwd, signal}),
  );
  if (v.sharing === null) return undefined;
  const sharing = record(v.sharing);
  if (typeof sharing.enabled !== 'boolean')
    throw new Error('Invalid dots share status');
  if (!sharing.enabled || v.changes === null) return undefined;
  const changes = string(v.changes);
  if (!path.isAbsolute(changes)) throw new Error('Invalid dots share status');
  return changes;
}
