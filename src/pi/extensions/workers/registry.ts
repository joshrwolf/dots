import { mkdirSync } from "node:fs";
import path from "node:path";
import type { DatabaseSync as DatabaseSyncType } from "node:sqlite";

// node:sqlite is stable enough for our use but still prints an ExperimentalWarning
// on Node 22; silence only that one message, and only around the import.
function loadSqlite(): typeof import("node:sqlite") {
  const original = process.emitWarning;
  process.emitWarning = ((warning: string | Error, ...rest: unknown[]) => {
    const text = typeof warning === "string" ? warning : warning.message;
    if (text.includes("SQLite")) return;
    (original as (...args: unknown[]) => void).call(process, warning, ...rest);
  }) as typeof process.emitWarning;
  try {
    // Synchronous builtin loading keeps the warning hook out of unrelated
    // async work and concurrent registry opens (supported since Node 22.3).
    return process.getBuiltinModule("node:sqlite") as typeof import("node:sqlite");
  } finally {
    process.emitWarning = original;
  }
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
  model: string | null;
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
  kind: "call" | "result" | "said" | "state" | "note";
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
  model text,
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
create index if not exists workers_parent on workers(parent);
create index if not exists workers_root on workers(root);
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
  private readonly db: DatabaseSyncType;
  private readonly upsertStmt;
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
  private closed = false;

  constructor(db: DatabaseSyncType) {
    this.db = db;
    this.db.exec("pragma busy_timeout = 2000; pragma journal_mode = wal; pragma synchronous = normal;");
    this.db.exec(SCHEMA);
    this.upsertStmt = this.db.prepare(`
      insert into workers (id, parent, root, owner_session, label, state, cwd, model, activity, failed, question, calls, tokens, started_at, last_event_at, updated_at, session_file, pid)
      values (@id, @parent, @root, @owner_session, @label, @state, @cwd, @model, @activity, @failed, @question, @calls, @tokens, @started_at, @last_event_at, @updated_at, @session_file, @pid)
      on conflict(id) do update set
        parent = excluded.parent, root = excluded.root, owner_session = excluded.owner_session, label = excluded.label, state = excluded.state,
        cwd = excluded.cwd, model = excluded.model, activity = excluded.activity, failed = excluded.failed, question = excluded.question,
        calls = excluded.calls, tokens = excluded.tokens, started_at = excluded.started_at, last_event_at = excluded.last_event_at,
        updated_at = excluded.updated_at, session_file = excluded.session_file, pid = excluded.pid`);
    this.eventStmt = this.db.prepare("insert into events (worker, at, kind, text) values (?, ?, ?, ?)");
    // Observation is a bounded recent trail, not a second transcript store.
    this.trimWorkerEvents = this.db.prepare("delete from events where worker = ? and seq <= (select seq from events where worker = ? order by seq desc limit 1 offset 200)");
    this.trimEvents = this.db.prepare("delete from events where seq <= (select seq from events order by seq desc limit 1 offset 10000)");
    this.childrenStmt = this.db.prepare("select * from workers where parent is ? order by started_at");
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
    this.eventsStmt = this.db.prepare("select worker, at, kind, text from events where worker = ? order by seq desc limit ?");
    this.rowStmt = this.db.prepare("select * from workers where id = ?");
    this.allStmt = this.db.prepare("select * from workers order by started_at");
    this.livePidsStmt = this.db.prepare("select distinct pid from workers where state in ('running','asking')");
    this.loseStmt = this.db.prepare("update workers set state = 'lost', updated_at = ? where pid = ? and state in ('running','asking')");
  }

  upsert(row: RegistryRow): void {
    this.upsertStmt.run({ ...row } as unknown as Record<string, string | number | null>);
  }

  event(event: ActivityEvent): void {
    this.eventStmt.run(event.worker, event.at, event.kind, event.text.slice(0, 2000));
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
  children(parent: string | null, alive: (pid: number) => boolean = processAlive): RegistryRow[] {
    this.sweep(alive);
    return this.childrenStmt.all(parent) as unknown as RegistryRow[];
  }

  /** Mark live rows owned by dead processes as lost. One signal-0 per distinct owner pid. */
  sweep(alive: (pid: number) => boolean = processAlive): number {
    const pids = (this.livePidsStmt.all() as { pid: number }[]).map((r) => r.pid);
    const dead = pids.filter((pid) => !alive(pid));
    let marked = 0;
    for (const pid of dead) marked += Number(this.loseStmt.run(Date.now(), pid).changes);
    return marked;
  }

  all(): RegistryRow[] {
    return this.allStmt.all() as unknown as RegistryRow[];
  }

  /** Descendants of `id`, not including `id` itself. Zero counts when none. */
  subtree(id: string, now: number, quietAfterMs: number, alive: (pid: number) => boolean = processAlive): SubtreeSummary {
    this.sweep(alive);
    const row = this.subtreeStmt.get(id, now, quietAfterMs) as Record<string, number | null> | undefined;
    return { live: row?.live ?? 0, asking: row?.asking ?? 0, failed: row?.failed ?? 0, quiet: row?.quiet ?? 0 };
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

function processAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === "EPERM";
  }
}

/** Opens the registry, or returns undefined when SQLite is unavailable or the file is unusable; observation is best-effort. */
export async function openRegistry(file: string, warn: (message: string) => void): Promise<Registry | undefined> {
  let db: DatabaseSyncType | undefined;
  try {
    const { DatabaseSync } = await loadSqlite();
    mkdirSync(path.dirname(file), { recursive: true });
    db = new DatabaseSync(file);
    return new Registry(db);
  } catch (error) {
    try { db?.close(); } catch { /* initialization failed before a usable handle */ }
    warn(`worker registry unavailable (${(error as Error).message}); fleet view disabled`);
    return undefined;
  }
}
