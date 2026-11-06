import {mkdirSync} from 'node:fs';
import path from 'node:path';
import {DatabaseSync} from 'node:sqlite';
import {getAgentDir} from '@earendil-works/pi-coding-agent';
import {isTimestamp} from '../../src/date.ts';
import {fields} from './protocol.ts';
import {Check} from 'typebox/value';
import {WorkerContextSchema, type WorkerContext} from './context.ts';

export interface ContextObservation {
  readonly snapshot: WorkerContext;
  readonly observedAt: number;
}

// Machine-local observation plane for workers: every runtime (root sessions
// and workers alike) writes the rows it owns; any runtime reads the tree under
// its own children. Authority for resume stays in the owner's session entries;
// nothing here may make a runtime act. One SQLite file, WAL, short writes.

export interface RegistryRow {
  id: string;
  parent: string | null;
  root: string;
  owner_session: string | null;
  label: string;
  state: string;
  cwd: string;
  task_project: string | null;
  task_id: string | null;
  model: string | null;
  thinking: string | null;
  activity: string | null;
  failed: string | null;
  question: string | null;
  calls: number;
  tokens: number;
  started_at: number;
  last_event_at: number;
  updated_at: number;
  session_file: string | null;
  pid: number;
}

export interface ActivityEvent {
  worker: string;
  at: number;
  kind: 'call' | 'result' | 'said' | 'state' | 'note';
  text: string;
}

/** Live/asking counts for a subtree, as shown on a child's row. */
export interface SubtreeSummary {
  live: number;
  asking: number;
  failed: number;
  quiet: number;
}

const SCHEMA = `
create table if not exists workers (
  id text primary key,
  parent text,
  root text not null,
  owner_session text,
  label text not null,
  state text not null,
  cwd text not null,
  task_project text,
  task_id text,
  model text,
  thinking text,
  activity text,
  failed text,
  question text,
  calls integer not null default 0,
  tokens integer not null default 0,
  started_at integer not null,
  last_event_at integer not null,
  updated_at integer not null,
  session_file text,
  pid integer not null
);
create index if not exists workers_task on workers(task_project, task_id);
create index if not exists workers_parent on workers(parent);
create index if not exists workers_root on workers(root);
create table if not exists worker_context (
  worker text primary key,
  model text not null,
  tokens integer,
  context_window integer not null,
  compact_at integer,
  compactions integer not null,
  observed_at integer not null
);
create table if not exists events (
  seq integer primary key autoincrement,
  worker text not null,
  at integer not null,
  kind text not null,
  text text not null
);
create index if not exists events_worker on events(worker, seq);
`;

export class Registry {
  private readonly db: DatabaseSync;
  private readonly upsertStmt;
  private readonly contextWriteStmt;
  private readonly contextReadStmt;
  private readonly eventStmt;
  private readonly childrenStmt;
  private readonly subtreeStmt;
  private readonly eventsStmt;
  private readonly rowStmt;
  private readonly allStmt;
  private readonly livePidsStmt;
  private readonly loseStmt;
  private readonly trimWorkerEvents;
  private readonly trimEvents;
  private readonly forgetStmts;
  private closed = false;

  constructor(db: DatabaseSync) {
    this.db = db;
    this.db.exec(
      'pragma busy_timeout = 2000; pragma journal_mode = wal; pragma synchronous = normal;',
    );
    this.db.exec(SCHEMA);
    this.upsertStmt = this.db.prepare(`
      insert into workers (id, parent, root, owner_session, label, state, cwd, task_project, task_id, model, thinking, activity, failed, question, calls, tokens, started_at, last_event_at, updated_at, session_file, pid)
      values (@id, @parent, @root, @owner_session, @label, @state, @cwd, @task_project, @task_id, @model, @thinking, @activity, @failed, @question, @calls, @tokens, @started_at, @last_event_at, @updated_at, @session_file, @pid)
      on conflict(id) do update set
        parent = excluded.parent, root = excluded.root, owner_session = excluded.owner_session, label = excluded.label, state = excluded.state,
        cwd = excluded.cwd, task_project = excluded.task_project, task_id = excluded.task_id, model = excluded.model, thinking = excluded.thinking, activity = excluded.activity, failed = excluded.failed, question = excluded.question,
        calls = excluded.calls, tokens = excluded.tokens, started_at = excluded.started_at, last_event_at = excluded.last_event_at,
        updated_at = excluded.updated_at, session_file = excluded.session_file, pid = excluded.pid`);
    this.contextWriteStmt = this.db.prepare(`
      insert into worker_context (worker, model, tokens, context_window, compact_at, compactions, observed_at)
      values (?, ?, ?, ?, ?, ?, ?)
      on conflict(worker) do update set model=excluded.model, tokens=excluded.tokens,
        context_window=excluded.context_window, compact_at=excluded.compact_at,
        compactions=excluded.compactions, observed_at=excluded.observed_at`);
    this.contextReadStmt = this.db.prepare(
      'select * from worker_context where worker = ?',
    );
    this.eventStmt = this.db.prepare(
      'insert into events (worker, at, kind, text) values (?, ?, ?, ?)',
    );
    // Observation is a bounded recent trail, not a second transcript store.
    this.trimWorkerEvents = this.db.prepare(
      'delete from events where worker = ? and seq <= (select seq from events where worker = ? order by seq desc limit 1 offset 200)',
    );
    this.trimEvents = this.db.prepare(
      'delete from events where seq <= (select seq from events order by seq desc limit 1 offset 10000)',
    );
    this.childrenStmt = this.db.prepare(
      'select * from workers where parent is ? order by started_at',
    );
    this.subtreeStmt = this.db.prepare(`
      with recursive tree(id) as (
        select id from workers where parent = ?
        union
        select w.id from workers w join tree on w.parent = tree.id
      )
      select
        sum(case when w.state in ('running','asking') then 1 else 0 end) as live,
        sum(case when w.state = 'asking' then 1 else 0 end) as asking,
        sum(case when w.failed is not null and w.state = 'running' then 1 else 0 end) as failed,
        sum(case when w.state = 'running' and ? - w.last_event_at >= ? then 1 else 0 end) as quiet
      from workers w join tree on w.id = tree.id`);
    this.eventsStmt = this.db.prepare(
      'select worker, at, kind, text from events where worker = ? order by seq desc limit ?',
    );
    this.rowStmt = this.db.prepare('select * from workers where id = ?');
    this.allStmt = this.db.prepare('select * from workers order by started_at');
    this.livePidsStmt = this.db.prepare(
      "select distinct pid from workers where state in ('running','asking')",
    );
    this.forgetStmts = [
      'workers where id',
      'worker_context where worker',
      'events where worker',
    ].map(table => this.db.prepare(`delete from ${table} = ?`));
    this.loseStmt = this.db.prepare(
      "update workers set state = 'lost', updated_at = ? where pid = ? and state in ('running','asking')",
    );
  }

  upsert(row: RegistryRow): void {
    this.upsertStmt.run({...row} as unknown as Record<
      string,
      string | number | null
    >);
  }

  /** Only child telemetry updates this timestamp; ordinary activity cannot refresh stale context. */
  recordContext(worker: string, snapshot: WorkerContext, now: number): void {
    this.contextWriteStmt.run(
      worker,
      snapshot.model,
      snapshot.tokens,
      snapshot.window,
      snapshot.compactAt,
      snapshot.compactions,
      now,
    );
  }

  context(worker: string): ContextObservation | undefined {
    const row = this.contextReadStmt.get(worker);
    if (!row) return undefined;
    const snapshot = {
      model: row.model,
      tokens: row.tokens,
      window: row.context_window,
      compactAt: row.compact_at,
      compactions: row.compactions,
    };
    if (
      !Check(WorkerContextSchema, snapshot) ||
      !isTimestamp(row.observed_at, 'milliseconds')
    )
      throw new Error(`Invalid context observation for worker ${worker}`);
    return {snapshot, observedAt: row.observed_at};
  }

  event(event: ActivityEvent): void {
    this.eventStmt.run(
      event.worker,
      event.at,
      event.kind,
      event.text.slice(0, 2000),
    );
    this.trimWorkerEvents.run(event.worker, event.worker);
    this.trimEvents.run();
  }

  get(id: string): RegistryRow | undefined {
    return this.rowStmt.get(id) as RegistryRow | undefined;
  }

  /**
   * Direct children. A row whose owner process is gone cannot be running: a
   * hard-killed owner never got to mark it, so it reads as `lost` here (and is
   * written back so other readers and the tree query agree).
   */
  children(
    parent: string | null,
    alive: (pid: number) => boolean = processAlive,
  ): RegistryRow[] {
    this.sweep(alive);
    return this.childrenStmt.all(parent) as unknown as RegistryRow[];
  }

  /** Mark live rows owned by dead processes as lost. One signal-0 per distinct owner pid. */
  sweep(alive: (pid: number) => boolean = processAlive): number {
    const pids = (this.livePidsStmt.all() as {pid: number}[]).map(r => r.pid);
    const dead = pids.filter(pid => !alive(pid));
    let marked = 0;
    for (const pid of dead)
      marked += Number(this.loseStmt.run(Date.now(), pid).changes);
    return marked;
  }

  /** Whether a live process owns this worker's row. */
  ownerAlive(
    id: string,
    alive: (pid: number) => boolean = processAlive,
  ): boolean {
    const row = this.get(id);
    return row !== undefined && alive(row.pid);
  }

  /** Removes every row about a worker whose session is gone. */
  forget(id: string): void {
    for (const statement of this.forgetStmts) statement.run(id);
  }

  all(): RegistryRow[] {
    return this.allStmt.all() as unknown as RegistryRow[];
  }

  /** Descendants of `id`, not including `id` itself. Zero counts when none. */
  subtree(
    id: string,
    now: number,
    quietAfterMs: number,
    alive: (pid: number) => boolean = processAlive,
  ): SubtreeSummary {
    this.sweep(alive);
    const row = this.subtreeStmt.get(id, now, quietAfterMs) as
      Record<string, number | null> | undefined;
    return {
      live: row?.live ?? 0,
      asking: row?.asking ?? 0,
      failed: row?.failed ?? 0,
      quiet: row?.quiet ?? 0,
    };
  }

  /** Most recent first. */
  events(worker: string, limit: number): ActivityEvent[] {
    return this.eventsStmt.all(worker, limit) as unknown as ActivityEvent[];
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.db.close();
  }
}

export function registryFile(): string {
  return path.join(getAgentDir(), 'dots/workers.db');
}

/**
 * Lock contention after the busy timeout (SQLITE_BUSY, SQLITE_LOCKED and their
 * extended codes): skip one observation and keep the registry. Anything else
 * means the database itself is unusable.
 */
export function isContention(error: unknown): boolean {
  const errcode = fields(error)?.errcode;
  return (
    typeof errcode === 'number' &&
    ((errcode & 0xff) === 5 || (errcode & 0xff) === 6)
  );
}

/** One task contributor as another runtime recorded it; `updated_at` is epoch milliseconds. */
export interface TaskWorker {
  readonly id: string;
  readonly parent: string | null;
  readonly owner_session: string | null;
  readonly task_id: string;
  readonly label: string;
  readonly state: string;
  readonly cwd: string;
  readonly updated_at: number;
  readonly session_file: string | null;
  readonly ownerAvailable: boolean;
}
export interface TaskWorkers {
  readonly rows: readonly TaskWorker[];
  readonly truncated: boolean;
  readonly warning?: string;
}

const nullableText = (value: unknown): value is string | null =>
  value === null || typeof value === 'string';

/** A row other runtimes wrote, decoded at this cross-extension boundary. */
function taskWorker(value: unknown): TaskWorker | undefined {
  const row = fields(value);
  if (!row) return undefined;
  const {
    id,
    parent,
    owner_session,
    task_id,
    label,
    state,
    cwd,
    updated_at,
    session_file,
    pid,
  } = row;
  if (
    typeof id !== 'string' ||
    typeof task_id !== 'string' ||
    typeof label !== 'string' ||
    typeof state !== 'string' ||
    typeof cwd !== 'string'
  )
    return undefined;
  if (
    !nullableText(parent) ||
    !nullableText(owner_session) ||
    !nullableText(session_file)
  )
    return undefined;
  if (
    !isTimestamp(updated_at, 'milliseconds') ||
    typeof pid !== 'number' ||
    !Number.isSafeInteger(pid) ||
    pid <= 0
  )
    return undefined;
  return {
    id,
    parent,
    owner_session,
    task_id,
    label,
    state,
    cwd,
    updated_at,
    session_file,
    ownerAvailable: processAlive(pid),
  };
}

/** Read-only, bounded cross-session observations. Missing data is not an empty fleet. */
export function readTaskWorkers(
  project: string,
  ids: readonly string[],
  file = registryFile(),
): TaskWorkers {
  let db: DatabaseSync | undefined;
  try {
    db = new DatabaseSync(file, {readOnly: true});
    db.exec('pragma busy_timeout = 100;');
    const rows = db
      .prepare(
        `select id, parent, owner_session, task_id, label, state, cwd, updated_at, session_file, pid
      from workers where task_project = ? and task_id in (select value from json_each(?))
      order by updated_at desc, id limit 21`,
      )
      .all(project, JSON.stringify(ids));
    const decoded = rows.slice(0, 20).map(taskWorker);
    const valid = decoded.filter(row => row !== undefined);
    const malformed = decoded.length - valid.length;
    return {
      rows: valid,
      truncated: rows.length > 20,
      ...(malformed
        ? {
            warning: `${malformed} worker observation${malformed === 1 ? ' was' : 's were'} malformed and omitted`,
          }
        : {}),
    };
  } catch (error) {
    return {
      rows: [],
      truncated: false,
      warning: `Worker observations unavailable: ${String(error).slice(0, 512)}`,
    };
  } finally {
    db?.close();
  }
}

function processAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return fields(error)?.code === 'EPERM';
  }
}

/** Opens the registry, or returns undefined when the file is unusable; observation is best-effort. */
export function openRegistry(
  file: string,
  warn: (message: string) => void,
): Registry | undefined {
  let db: DatabaseSync | undefined;
  try {
    mkdirSync(path.dirname(file), {recursive: true});
    db = new DatabaseSync(file);
    return new Registry(db);
  } catch (error) {
    try {
      db?.close();
    } catch {
      /* initialization failed before a usable handle */
    }
    warn(`worker registry unavailable (${String(error)}); fleet view disabled`);
    return undefined;
  }
}
