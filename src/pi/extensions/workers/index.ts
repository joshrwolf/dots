import {randomBytes} from 'node:crypto';
import {existsSync, mkdirSync} from 'node:fs';
import {readdir, rm, stat} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {
  getAgentDir,
  type AgentToolResult,
  type ExtensionAPI,
  type ExtensionContext,
  type SessionEntry,
} from '@earendil-works/pi-coding-agent';
import {
  THINKING_LEVELS,
  refOf,
  type CatalogEntry,
} from '@joshrwolf/pi-kit/models';
import {Type} from 'typebox';
import {labelText} from '../../src/index.ts';
import {
  assignmentContext,
  verifyAssignment,
  type TaskAssignment,
} from './assignment.ts';
import {CHILD_CONTRACT, loadConfig} from './config.ts';
import {
  selectableSkills,
  selectSkills,
  skillContext,
  type SkillRef,
} from './skills.ts';
import {
  CHOICE_GUIDANCE,
  buildCatalog,
  describeCatalog,
  resolveChoice,
} from './models.ts';
import {
  DEFAULT_WIDGET_LINES,
  WORKER_FRAME_MS,
  STALL_MS,
  attentionOrder,
  compact,
  costDetail,
  createWidget,
  describeActivity,
  describeStep,
  describeText,
  type EventDetails,
  type WidgetRow,
} from './present.ts';
import {thinkingHeading} from '../../src/features/thinking.ts';
import {
  registryFile,
  isContention,
  openRegistry,
  type Registry,
} from './registry.ts';
import {workerEnv} from './env.ts';
import {PiProcess} from './rpc.ts';
import {
  asError,
  fields,
  type UiAnswer,
  type UiRequest,
  type WorkerEvent,
} from './protocol.ts';
import {
  RECORD_VERSION,
  RestoreReport,
  WORKER_ENTRY,
  WorkerRecordSchema,
  decode,
  type EventKind,
  type State,
  type WorkerRecord,
} from './record.ts';
import {publishActivity, recordId} from '../../src/host/activity.ts';
import {currentAgent} from '../../src/host/agent.ts';
import {summaryLine} from '../../src/host/summary.ts';
import {
  WORKER_ACTIVITY_EVENT,
  projectWorkerActivity,
} from '../../src/ui/worker-activity.ts';
import {WorkerInspection} from './inspection.ts';
import {createCheckoutResolver} from './checkout.ts';
import {type WorkerInspectionSource, type WorkerListItem} from './inspector.ts';
import {transcriptTail} from './tail.ts';
import {transcriptOptions} from './transcript.ts';
import {rexDo} from '../rex/index.ts';
import {
  addCost,
  assistantResponse,
  settledOutcome,
  type AssistantResponse,
} from './outcome.ts';
import {WorkerDelivery} from './outbox.ts';
import {
  contextLabel,
  decodeWorkerContext,
  registerWorkerContext,
  workerMeter,
  observedContext,
} from './context.ts';

// Workers: headless Pi children with their own context and session. Five
// primitives: spawn, message, wait, status, stop; blocking waits exist only
// outside the interactive TUI. `message` is the only communication primitive
// and is not directional: any session sends to any address it can see
// (`parent`, or a worker it owns, or a sibling by id via the owner). Delivery adapts to the target's state: answered if it is blocked
// waiting, steered if running, continued in its own session if finished.
// A settled worker's process exits once its own workers have settled; the next
// message relaunches it on its saved session. Return is the event: a worker's settle, question, or message arrives as a
// custom message that starts the owner's next turn.
//
// Workers are resources of the session that spawned them, not of a branch:
// /tree navigation keeps every worker running and addressable, and reload
// rebuilds them from all of the session's entries. Records copied into a fork
// or clone carry the original session id; there they are read-only history.

/** A blocking dialog the child is waiting on; answered by the next `message`. */
interface Question {
  uiId: string;
  text: string;
  method: string;
  deadline?: number;
}

/**
 * A worker as this runtime holds it: its persisted record (see record.ts)
 * plus attachments that never outlive the runtime. Optional fields are absent
 * until known; `undefined` clears them.
 */
interface Worker {
  // Persisted.
  readonly id: string;
  /** The session that owns this worker; an inherited record names another. */
  readonly owner: string;
  label: string;
  cwd: string;
  sessionDir: string;
  sessionFile?: string | undefined;
  model?: string | undefined;
  thinking?: string | undefined;
  tools?: string[] | undefined;
  system?: string | undefined;
  assignment?: TaskAssignment | undefined;
  /** Skills the parent selected, as the host named and located them at spawn. */
  skills?: SkillRef[] | undefined;
  state: State;
  startedAt: number;
  updatedAt: number;
  /** When it last completed, failed, or stopped; the widget keeps it a while after. */
  finishedAt?: number | undefined;
  calls: number;
  tokens: number;
  /** Pi's reported cost total for its responses so far, in dollars. */
  cost?: number | undefined;
  /** Set only while every counted response was provider-billed; otherwise the total is an estimate. */
  costBilled?: true | undefined;
  /** Tokens its last response used, as an estimate of its context; null right after compaction. */
  contextTokens?: number | null | undefined;
  /** The chosen model's context window. */
  contextWindow?: number | undefined;
  /** Child-reported native trigger; null means disabled, absent means not yet reported. */
  compactAt?: number | null | undefined;
  /** Child-reported compactions on its active branch; absent until reported. */
  compactions?: number | undefined;
  /** Counts settlements independently of prompt generation (including spontaneous turns). */
  settlements: number;
  result?: string | undefined;
  /** Text from an incomplete run, never presented as a final result. */
  partial?: string | undefined;
  error?: string | undefined;

  // Runtime only.
  process?: PiProcess | undefined;
  /** True from our prompt/message until the first settle answers it; later
   * spontaneous settles (the child's own children waking it) are updates. */
  awaiting: boolean;
  /** Pending dialogs, oldest first. */
  questions: Question[];
  question?: Question | undefined;
  /** Bumped by every deliver/stop so an in-flight settle can tell it is stale. */
  generation: number;
  /** The assistant response from this run, captured at message_end. */
  lastResponse?: AssistantResponse | undefined;
  tool?: string | undefined;
  /** Current call, or the last thing said/done between calls (glance text). */
  activity?: string | undefined;
  /** The same moment in words for status records, without raw commands. */
  step?: string | undefined;
  failed?: string | undefined;
  lastEventAt: number;
  /** Explicit progress takes precedence over reasoning headings within one run. */
  headline?: string | undefined;
  headlineSource?: 'reasoning' | 'progress' | undefined;
  /** Compacting or retrying, while it lasts. */
  phase?: string | undefined;
}

const TICK_MS = 5_000;
/** How long a finished worker stays in the widget. */
const RECENT_MS = 120_000;
/** How long a finished worker stays in default listings; `all` lists every one. */
const LISTED_MS = 3_600_000;
/** A finished worker's process closes like a stop: grace, then escalation. */
const CLOSE_GRACE_MS = 2000;
/** Full result kept on the worker and in its session file. */
const MAX_RESULT = 12_000;
/** What an event carries into the owner's context; the rest is one status call away. */
const EVENT_CHARS = 1_500;
const EVENT_LINES = 12;
/** Session shutdown has no host deadline; bound our cleanup (a close's grace and kill escalation). */
const SHUTDOWN_MS = 5_000;
/** Message and wait windows; setTimeout's own bound is about 24.8 days. */
const MAX_WAIT_MS = 86_400_000;

/** Bound a result for the event body without losing its lead: outcome first. */
function excerpt(text: string): {body: string; truncated: boolean} {
  const lines = text.split('\n');
  let body = lines.slice(0, EVENT_LINES).join('\n');
  if (body.length > EVENT_CHARS) body = body.slice(0, EVENT_CHARS);
  const truncated = body.length < text.length;
  return {body: truncated ? `${body.trimEnd()}\n…` : body, truncated};
}
/** Prefix for messages a child sends upward through the extension-UI notify channel. */
const MARK = 'dots.message:';

/** The read-only transcript viewer a Rex view runs; see the Rex `workers_view` action. */
const VIEWER = fileURLToPath(
  new URL('../worker-view/index.ts', import.meta.url),
);

/** Opens a worker's saved transcript in a Rex view on the server that owns this terminal. */
export async function viewInRex(
  pi: Pick<ExtensionAPI, 'exec'>,
  sessionFile: string,
  label: string,
  env: NodeJS.ProcessEnv = process.env,
): Promise<void> {
  await rexDo(
    pi,
    'workers_view',
    {session_file: path.resolve(sessionFile), viewer: VIEWER, label},
    env,
  );
}

/** This session's nesting depth. We set the variable ourselves, so a malformed value is a broken invariant, not depth 0. */
function depth(env: NodeJS.ProcessEnv = process.env): number {
  const raw = env.DOTS_WORKER_DEPTH;
  if (raw === undefined || raw === '') return 0;
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value < 0)
    throw new Error(
      `DOTS_WORKER_DEPTH must be a non-negative integer, not ${JSON.stringify(raw.slice(0, 32))}; refusing to spawn without a nesting guard`,
    );
  return value;
}

/** Set in worker sessions; `parent` is a valid address only there. */
function selfId(env: NodeJS.ProcessEnv = process.env): string | undefined {
  return env.DOTS_WORKER_ID || undefined;
}

/** Root worker id of the tree this session belongs to; set by the spawner. */
function rootId(env: NodeJS.ProcessEnv = process.env): string | undefined {
  return env.DOTS_WORKER_ROOT || undefined;
}

function sessionsRoot(): string {
  return path.join(getAgentDir(), 'dots/workers');
}

/** How long a worker's session outlives its last write; `message` can resume it until then. */
export const RETENTION_MS = 30 * 86_400_000;

const missing = (error: unknown) => fields(error)?.code === 'ENOENT';

/**
 * Removes worker session directories under `root` last written before the
 * retention period, except those `keep` names. Resolves with the removed
 * worker ids. Another runtime pruning the same directory is not an error.
 */
export async function pruneSessions(
  root: string,
  keep: (id: string) => boolean,
  now = Date.now(),
): Promise<string[]> {
  let ids: string[];
  try {
    ids = await readdir(root);
  } catch (error) {
    if (missing(error)) return [];
    throw error;
  }
  const removed: string[] = [];
  for (const id of ids) {
    const directory = path.join(root, id);
    try {
      const files = await readdir(directory);
      const times = await Promise.all(
        [directory, ...files.map(file => path.join(directory, file))].map(
          async file => (await stat(file)).mtimeMs,
        ),
      );
      if (now - Math.max(...times) < RETENTION_MS || keep(id)) continue;
      await rm(directory, {recursive: true, force: true});
      removed.push(id);
    } catch (error) {
      if (!missing(error)) throw error;
    }
  }
  return removed;
}

/** One line of untrusted text for a listing; marks its own truncation. */
function label(text: string, max = 60): string {
  const clean = labelText(text);
  return clean.length > max ? `${clean.slice(0, max - 1)}\u2026` : clean;
}

function duration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m${s % 60 ? ` ${s % 60}s` : ''}`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

/** Share of its context window a worker is using, when known. */
function contextShare(
  worker: Pick<Worker, 'contextTokens' | 'contextWindow'>,
): number | undefined {
  return typeof worker.contextTokens === 'number' && worker.contextWindow
    ? (worker.contextTokens / worker.contextWindow) * 100
    : undefined;
}

function elapsed(since: number): string {
  const seconds = Math.max(0, Math.round((Date.now() - since) / 1000));
  return seconds < 60
    ? `${seconds}s`
    : seconds < 3600
      ? `${Math.round(seconds / 60)}m`
      : `${(seconds / 3600).toFixed(1)}h`;
}

const live = (worker: Pick<Worker, 'state'>) =>
  worker.state === 'running' || worker.state === 'asking';

/** Launch arguments shared by a new child and a continued one, so a worker
 * keeps its model, tools, and contract across reload. The nesting guard
 * removes only spawning. Reads the selected skill files (see skillContext). */
function launchArgs(
  worker: Pick<
    Worker,
    | 'label'
    | 'model'
    | 'thinking'
    | 'tools'
    | 'system'
    | 'assignment'
    | 'skills'
  >,
  childDepth: number,
  maxDepth: number,
): string[] {
  const args = [
    '--name',
    worker.label,
    '--append-system-prompt',
    [
      worker.system?.trim(),
      CHILD_CONTRACT,
      skillContext(worker.skills),
      assignmentContext(worker.assignment),
    ]
      .filter(Boolean)
      .join('\n\n'),
  ];
  if (childDepth >= maxDepth) args.push('--exclude-tools', 'worker_spawn');
  if (worker.model) args.push('--model', worker.model);
  if (worker.thinking) args.push('--thinking', worker.thinking);
  if (worker.tools)
    args.push('--tools', [...new Set([...worker.tools, 'message'])].join(','));
  // MCP tools are reachable only through codemode or tool_search; without either, skip connecting.
  if (
    worker.tools &&
    !worker.tools.includes('codemode') &&
    !worker.tools.includes('tool_search')
  )
    args.push('--no-mcp');
  return args;
}

/** How to answer each blocking dialog method, per Pi's extension-UI protocol. */
function uiAnswer(method: string, text: string): UiAnswer {
  if (method === 'confirm')
    return {confirmed: /^(?:y|yes|true|ok|confirm)$/i.test(text.trim())};
  return {value: text};
}

/** `value` without its undefined fields: absent, as the persisted schema has them. */
function present<T extends object>(
  value: T,
): {[K in keyof T]?: Exclude<T[K], undefined>} {
  return Object.fromEntries(
    Object.entries(value).filter(([, field]) => field !== undefined),
  ) as {[K in keyof T]?: Exclude<T[K], undefined>};
}

/** The persisted allowlist; runtime attachments never reach the session file. */
function persisted(worker: Worker): WorkerRecord {
  return {
    version: RECORD_VERSION,
    owner: worker.owner,
    id: worker.id,
    label: worker.label,
    cwd: worker.cwd,
    sessionDir: worker.sessionDir,
    state: worker.state,
    startedAt: worker.startedAt,
    updatedAt: worker.updatedAt,
    calls: worker.calls,
    tokens: worker.tokens,
    settlements: worker.settlements,
    ...present({
      sessionFile: worker.sessionFile,
      model: worker.model,
      thinking: worker.thinking,
      tools: worker.tools,
      system: worker.system,
      assignment: worker.assignment,
      skills: worker.skills,
      finishedAt: worker.finishedAt,
      cost: worker.cost,
      costBilled: worker.costBilled,
      contextTokens: worker.contextTokens,
      contextWindow: worker.contextWindow,
      compactAt: worker.compactAt,
      compactions: worker.compactions,
      result: worker.result,
      partial: worker.partial,
      error: worker.error,
    }),
  };
}

/** A worker as a restored record describes it, with fresh runtime attachments. */
function restored(record: WorkerRecord): Worker {
  const {version: _version, ...fieldsOf} = record;
  return {
    ...fieldsOf,
    tools: record.tools ? [...record.tools] : undefined,
    skills: record.skills?.map(skill => ({...skill})),
    questions: [],
    awaiting: false,
    generation: 0,
    lastEventAt: Date.now(),
  };
}

/** Test seam: the Pi a worker runs (default: `pi` on PATH). */
export interface WorkersOptions {
  command?: string[];
}

export default function (pi: ExtensionAPI, options: WorkersOptions = {}) {
  const workers = new Map<string, Worker>();
  // Serialize transitions per worker, never across the fleet. Stop bypasses
  // this queue and invalidates queued work so an unresponsive child is stoppable.
  const transitions = new WeakMap<Worker, Promise<unknown>>();
  const epochs = new WeakMap<Worker, number>();
  const stopping = new WeakMap<Worker, Promise<void>>();
  const inOrder = <T>(
    worker: Worker,
    operation: () => Promise<T>,
  ): Promise<T> => {
    const epoch = epochs.get(worker) ?? 0;
    if (stopping.has(worker))
      return Promise.reject(
        new Error(`${worker.id} is stopping; retry after it stops`),
      );
    // An earlier transition's failure belongs to its own caller; this one runs regardless.
    const next = (transitions.get(worker) ?? Promise.resolve())
      .catch(() => {})
      .then(() => {
        if (shuttingDown || epoch !== (epochs.get(worker) ?? 0))
          throw new Error(
            `${worker.id} operation cancelled by stop or shutdown`,
          );
        return operation();
      });
    transitions.set(worker, next);
    const clean = () => {
      if (transitions.get(worker) === next) transitions.delete(worker);
    };
    next.then(clean, clean);
    return next;
  };
  // Contexts are invalidated when the runtime is replaced (reload, resume), so
  // keep the most recent one from any event or tool call for widget updates.
  const inspection = new WorkerInspection();
  let context: ExtensionContext | undefined;
  /** This runtime's session; a worker whose record names another is inherited history. */
  let sessionId: string | undefined;
  let shuttingDown = false;
  const warn = (message: string) => {
    if (shuttingDown) return;
    if (context?.hasUI) context.ui.notify(message, 'warning');
    else console.error(message);
  };
  const delivery = new WorkerDelivery(
    pi,
    () => [...workers.values()].some(worker => worker.state === 'running'),
    () => !shuttingDown,
  );
  const outbox = delivery.outbox;
  const checkoutResolver = createCheckoutResolver(
    (command, args, execOptions) => pi.exec(command, args, execOptions),
    () => {
      if (shuttingDown) return;
      widget();
      inspection.refresh();
    },
    error => {
      warn(
        `worker checkout observation failed: ${labelText(String(error)).slice(0, 256)}`,
      );
    },
  );
  const checkoutCue = (cwd: string) =>
    context ? checkoutResolver.get(cwd, context.cwd) : undefined;
  let maxWidgetLines: number = DEFAULT_WIDGET_LINES;
  let parentQuestionPending = false;
  const track = (ctx: ExtensionContext) => {
    if (!shuttingDown) context = ctx;
    for (const worker of workers.values()) refreshQuestions(worker);
  };
  const owned = (worker: Worker) => worker.owner === sessionId;
  const ownerSession = (): string => {
    if (sessionId === undefined)
      throw new Error('the worker runtime has no session yet');
    return sessionId;
  };
  /**
   * This runtime's task actor, when the bound agent is this session's; read
   * synchronously where a dispatch starts so no later runtime is adopted.
   */
  const runtimeActor = (): string | undefined => {
    const bound = currentAgent();
    return bound !== undefined && bound.session === sessionId
      ? bound.owner
      : undefined;
  };
  /** Inherited workers belong to the session this one was forked or cloned from. */
  const inherited = (worker: Worker): Error =>
    new Error(
      `${worker.id} belongs to session ${worker.owner}, which this session was forked or cloned from; it is read-only history here. Spawn a new worker to continue that work.`,
    );

  // Observation plane: best-effort, never authoritative. Opened lazily so an
  // unusable database only disables the fleet view.
  let registry: Registry | undefined;
  let registryOpened = false;
  const ensureRegistry = () => {
    if (registryOpened) return;
    registryOpened = true;
    registry = openRegistry(registryFile(), warn);
  };
  const withRegistry = <T>(read: (registry: Registry) => T): T | undefined => {
    if (!registry) return undefined;
    try {
      return read(registry);
    } catch (error) {
      // Another runtime holds the write lock past the busy timeout: skip this
      // observation and keep the registry. Anything else disables it.
      if (isContention(error)) return undefined;
      const failed = registry;
      registry = undefined;
      try {
        failed.close();
      } catch {
        /* already unavailable */
      }
      warn(`worker registry disabled: ${String(error)}`);
      return undefined;
    }
  };
  const observe = (worker: Worker) => {
    // An inherited record's row belongs to its own session's runtime.
    if (!owned(worker)) return;
    withRegistry(r =>
      r.upsert({
        id: worker.id,
        parent: selfId() ?? null,
        root: rootId() ?? worker.id,
        owner_session: context?.sessionManager.getSessionFile() ?? null,
        label: worker.label,
        state: worker.state,
        cwd: worker.cwd,
        model: worker.model ?? null,
        thinking: worker.thinking ?? null,
        task_project: worker.assignment?.project ?? null,
        task_id: worker.assignment?.id ?? null,
        activity: worker.activity ?? null,
        failed: worker.failed ?? null,
        question: worker.question?.text ?? null,
        calls: worker.calls,
        tokens: worker.tokens,
        started_at: worker.startedAt,
        last_event_at: worker.lastEventAt,
        updated_at: Date.now(),
        session_file: worker.sessionFile ?? null,
        pid: process.pid,
      }),
    );
  };
  const log = (
    worker: Worker,
    kind: 'call' | 'result' | 'said' | 'state' | 'note',
    text: string,
  ) => {
    withRegistry(r => {
      r.event({worker: worker.id, at: Date.now(), kind, text});
    });
  };
  const subtree = (worker: Worker) =>
    withRegistry(r => r.subtree(worker.id, Date.now(), STALL_MS));

  // A settled worker's process closes once nothing depends on it. Closing ends
  // the child's own workers through its shutdown, so a worker whose
  // descendants still run keeps its process until they settle. Without the
  // registry, descendants are unobservable and the process closes.
  /** Settled workers whose processes wait for their descendants. */
  const held = new Set<Worker>();
  /** Held workers whose descendants were all settled at the previous tick. */
  const drained = new Set<Worker>();
  /** Processes closing after their worker settled. */
  const retiring = new WeakMap<Worker, Promise<void>>();
  const unhold = (worker: Worker) => {
    held.delete(worker);
    drained.delete(worker);
  };
  /**
   * Closes a settled worker's process, keeping its state and result. Runs as
   * its own transition, so a delivery queued first continues the live process
   * and one queued later relaunches the saved session.
   */
  const release = (worker: Worker): void => {
    const child = worker.process;
    if (!child || live(worker) || !owned(worker) || shuttingDown) return;
    inOrder(worker, async () => {
      if (worker.process !== child || live(worker)) return;
      if ((subtree(worker)?.live ?? 0) > 0) {
        held.add(worker);
        syncTicker();
        return;
      }
      unhold(worker);
      syncTicker();
      worker.process = undefined; // an expected exit; onExit ignores it
      const closing = child.close(CLOSE_GRACE_MS);
      retiring.set(worker, closing);
      try {
        await closing;
      } finally {
        if (retiring.get(worker) === closing) retiring.delete(worker);
      }
    }).catch(() => {
      // Cancelled by stop or shutdown, which close the process themselves.
    });
  };
  const reapHeld = () => {
    for (const worker of [...held]) {
      if (!worker.process || live(worker)) {
        unhold(worker);
        continue;
      }
      if ((subtree(worker)?.live ?? 0) > 0) {
        drained.delete(worker);
        continue;
      }
      // A descendant's settle reaches the registry just before the turn it
      // starts in its owner; one quiet tick lets that turn begin first.
      if (drained.has(worker)) release(worker);
      else drained.add(worker);
    }
  };

  /** The last state each worker logged, so the activity trail records changes, not every save. */
  const loggedState = new WeakMap<Worker, string>();
  const save = (worker: Worker) => {
    if (shuttingDown || !owned(worker)) return;
    worker.updatedAt = Date.now();
    if (live(worker)) worker.finishedAt = undefined;
    else if (worker.state !== 'paused') worker.finishedAt ??= worker.updatedAt;
    pi.appendEntry(WORKER_ENTRY, persisted(worker));
    const state =
      worker.state + (worker.error ? `: ${worker.error.slice(0, 200)}` : '');
    if (loggedState.get(worker) !== state) {
      loggedState.set(worker, state);
      log(worker, 'state', state);
    }
    observe(worker);
    widget();
  };

  const descendantContext = (id: string, now: number, ownerLost = false) =>
    observedContext(
      withRegistry(r => r.context(id)),
      now,
      ownerLost,
    );

  // Widget rows: workers this session owns, each followed by its live
  // descendants from the registry (other runtimes' children), depth-first.
  const descendantRows = (
    parent: string,
    level: number,
    now: number,
    seen = new Set<string>(),
  ): WidgetRow[] => {
    if (seen.has(parent)) return [];
    seen.add(parent);
    const rows = (withRegistry(r => r.children(parent)) ?? [])
      .map((r): WidgetRow & {id: string} => ({
        id: r.id,
        label: r.label,
        model: r.model ?? undefined,
        thinking: r.thinking ?? undefined,
        ...(r.task_project && r.task_id
          ? {assignment: {project: r.task_project, id: r.task_id}}
          : {}),
        state: r.state,
        depth: level,
        elapsed: elapsed(r.started_at),
        tokens: r.tokens,
        ...descendantContext(r.id, now, r.state === 'lost'),
        calls: r.calls,
        checkout: checkoutCue(r.cwd),
        detail:
          r.state === 'asking'
            ? `asking: ${r.question ?? ''}`
            : (r.activity ?? r.state),
        failed: r.failed ?? undefined,
        idleMs: now - r.last_event_at,
      }))
      .sort(attentionOrder);
    return rows.flatMap(row => {
      if (seen.has(row.id)) return [];
      const children = descendantRows(row.id, level + 1, now, seen);
      return row.state === 'running' ||
        row.state === 'asking' ||
        children.length
        ? [row, ...children]
        : [];
    });
  };
  const liveRows = (): WidgetRow[] => {
    for (const worker of workers.values()) refreshQuestions(worker);
    const now = Date.now();
    const own = [...workers.values()]
      .filter(owned)
      .map((w): WidgetRow & {id: string; worker: Worker} => ({
        id: w.id,
        worker: w,
        label: w.label,
        model: w.model,
        thinking: w.thinking,
        ...(w.assignment ? {assignment: w.assignment} : {}),
        state: w.state,
        depth: 0,
        elapsed: elapsed(w.startedAt),
        tokens: w.tokens,
        calls: w.calls,
        checkout: checkoutCue(w.cwd),
        detail:
          w.state === 'asking'
            ? `asking: ${w.question?.text ?? ''}`
            : (w.activity ?? 'starting'),
        headline: w.headline,
        phase: w.phase,
        ...workerMeter(w),
        compactions: w.compactions,
        failed: w.failed,
        idleMs: now - w.lastEventAt,
      }))
      .sort(attentionOrder);
    return own.flatMap(({worker: w, ...row}): WidgetRow[] => {
      const children = descendantRows(row.id, 1, now);
      if (!live(w)) {
        if (children.length)
          return [
            {...row, detail: `${row.state} · descendants active`},
            ...children,
          ];
        // A finished worker stays briefly with its outcome, so the screen keeps a trail of what just ended.
        if (w.finishedAt === undefined || now - w.finishedAt >= RECENT_MS)
          return [];
        const outcome = (
          w.state === 'completed'
            ? w.result
            : w.state === 'interrupted'
              ? w.partial
                ? `partial: ${w.partial}`
                : 'no response text recorded'
              : w.error
        )
          ?.split('\n')
          .find(line => line.trim());
        return [
          {
            ...row,
            finished: true,
            headline: undefined,
            phase: undefined,
            context: undefined,
            elapsed: duration(w.finishedAt - w.startedAt),
            detail: outcome ? label(outcome, 160) : row.state,
          },
        ];
      }
      return [row, ...children];
    });
  };

  // Elapsed and quiet times move without child events; tick while anything is
  // shown, or a held process waits on descendants in other processes.
  let ticker: ReturnType<typeof setInterval> | undefined;
  let rowsShown = false;
  const syncTicker = () => {
    const active = !shuttingDown && (rowsShown || held.size > 0);
    // Descendants change in other processes; the tick refreshes their rows too.
    if (active && !ticker)
      ticker = setInterval(() => {
        reapHeld();
        widget();
      }, TICK_MS);
    if (!active && ticker) {
      clearInterval(ticker);
      ticker = undefined;
    }
  };

  // The widget is mounted once per live period and reads current rows on each
  // render; the host owns its disposal.
  let widgetMounted = false;
  let widgetRows: WidgetRow[] = [];
  let animation: ReturnType<typeof setInterval> | undefined;
  let animationFrame = 0;
  const animate = (running: boolean) => {
    if (running && !animation)
      animation = setInterval(() => {
        animationFrame++;
        publishWorkerActivity();
        requestRender?.();
      }, WORKER_FRAME_MS);
    if (!running && animation) {
      clearInterval(animation);
      animation = undefined;
    }
  };
  let requestRender: (() => void) | undefined;
  const publishWorkerActivity = () => {
    pi.events.emit(
      WORKER_ACTIVITY_EVENT,
      projectWorkerActivity(widgetRows, animationFrame, STALL_MS),
    );
  };
  const widget = () => {
    // A worker waiting on its parent's answer is still active work, not a prompt for the user.
    // A finished worker whose own workers still run is still active work.
    const items = shuttingDown
      ? []
      : [...workers.values()].filter(owned).flatMap(w => {
          const own = live(w);
          const descendants = own ? 0 : (subtree(w)?.live ?? 0);
          if (!own && descendants === 0) return [];
          const message =
            w.state === 'asking'
              ? `waiting for parent: ${summaryLine(w.question?.text) ?? ''}`
              : own
                ? (w.phase ?? w.headline ?? w.step)
                : `waiting on ${descendants} worker${descendants === 1 ? '' : 's'}`;
          // The id, not the label: labels repeat and normalize alike.
          return [
            {
              id: recordId('worker', w.id),
              title: w.label,
              ...(message === undefined ? {} : {message}),
            },
          ];
        });
    publishActivity(pi.events, 'workers', items);
    if (shuttingDown || context?.mode !== 'tui') {
      animate(false);
      rowsShown = false;
      syncTicker();
      return;
    }
    const rows = liveRows();
    rowsShown = rows.length > 0;
    syncTicker();
    widgetRows = rows;
    publishWorkerActivity();
    animate(
      rows.some(row => row.state === 'running' && !row.failed && !row.finished),
    );
    if (rows.length === 0) {
      if (widgetMounted) context.ui.setWidget('dots.workers', undefined);
      widgetMounted = false;
      requestRender = undefined;
      return;
    }
    if (!widgetMounted) {
      context.ui.setWidget(
        'dots.workers',
        (tui, theme) => {
          requestRender = () => tui.requestRender();
          return createWidget(
            () => widgetRows,
            theme,
            maxWidgetLines,
            () => animationFrame,
          );
        },
        {placement: 'belowEditor'},
      );
      widgetMounted = true;
    } else {
      requestRender?.();
    }
  };
  // Waiters: resolved whenever a worker settles, asks, messages, or dies, so a
  // session with nothing else to do can block in worker_wait or message{wait}.
  const waiters = new Set<
    (worker: Worker, kind: EventKind, body: string) => boolean
  >();
  const cancelWaiters = new Set<() => void>();
  /** Returns true when some waiter took the outcome: the blocked caller already has the text. */
  const wake = (worker: Worker, kind: EventKind, body: string): boolean => {
    let consumed = false;
    for (const waiter of [...waiters])
      consumed = waiter(worker, kind, body) || consumed;
    return consumed;
  };

  const notify = (worker: Worker, kind: EventKind, full: string) => {
    if (shuttingDown) return;
    // A completion is a session-wide result: it is journaled even when a
    // blocked worker_wait or message {wait} takes it, so a branch that never
    // saw that tool result still receives it.
    const journaled = kind === 'completed' || kind === 'update';
    const consumed = wake(worker, kind, full);
    if (consumed && !journaled) return; // the tool result is the model-facing notification
    const {body, truncated} = excerpt(full);
    const more = truncated
      ? `\n\n[${full.length.toLocaleString('en-US')} chars; full text: worker_status { id: "${worker.id}" }]`
      : '';
    const header = `Worker ${worker.label} [${worker.id}] ${kind}${worker.assignment ? ` · task ${worker.assignment.id}` : ''}`;
    const meter = workerMeter(worker);
    const details: EventDetails = {
      id: worker.id,
      kind,
      label: worker.label,
      truncated,
      elapsed: elapsed(worker.startedAt),
      tokens: worker.tokens,
      ...present({
        assignment: worker.assignment,
        ...meter,
        cost: worker.cost,
        costBilled: worker.costBilled,
        session: worker.sessionFile,
      }),
    };
    const content = `${header}\n\n${body}${more}`;
    if (journaled) {
      const item = {
        key: `${worker.id}:${worker.settlements}`,
        content,
        details,
      };
      // The waiter's tool result is this branch's receipt.
      if (consumed) outbox.taken(item);
      else outbox.offer(item);
      return;
    }
    // Questions and failures cannot wait for the ordinary completion hold.
    pi.sendMessage(
      {customType: 'dots.worker.event', content, display: true, details},
      {triggerTurn: delivery.canWake(), deliverAs: 'steer'},
    );
  };

  /**
   * A progress note from a worker: it becomes the worker's headline and joins
   * the owner's context without waking it or interrupting a wait. Questions
   * that need an answer use `message` with `wait: true`.
   */
  const note = (worker: Worker, text: string) => {
    if (shuttingDown) return;
    worker.headline = label(
      text.split('\n').find(line => line.trim()) ?? text,
      80,
    );
    worker.headlineSource = 'progress';
    worker.lastEventAt = Date.now();
    log(worker, 'note', text);
    observe(worker);
    widget();
    const preview = excerpt(text);
    pi.sendMessage(
      {
        customType: 'dots.worker.event',
        content: `Worker ${worker.label} [${worker.id}] note\n\n${preview.body}${preview.truncated ? `\n\n[Note preview limited; inspect worker_status { id: "${worker.id}" } to locate the session file for further inspection; status previews are limited.]` : ''}`,
        display: false,
        details: {
          id: worker.id,
          kind: 'note',
          label: worker.label,
        } satisfies EventDetails,
      },
      {triggerTurn: false, deliverAs: 'steer'},
    );
  };

  const find = (id: string): Worker => {
    const wanted = id.trim();
    if (!wanted) throw new Error('a worker id or label is required');
    const exact = workers.get(wanted);
    if (exact) return exact;
    const matches = [...workers.values()].filter(
      w => w.id.startsWith(wanted) || w.label === wanted,
    );
    const [only] = matches;
    if (only && matches.length === 1) return only;
    if (matches.length > 1)
      throw new Error(
        `${wanted} is ambiguous: ${matches.map(w => `${w.id} (${w.label})`).join(', ')}`,
      );
    throw new Error(
      `no worker ${wanted}; known: ${[...workers.keys()].join(', ') || 'none'}`,
    );
  };
  /** A worker this session may act on: messages, stops, and waits refuse inherited history. */
  const findOwned = (id: string): Worker => {
    const worker = find(id);
    if (!owned(worker)) throw inherited(worker);
    return worker;
  };

  const refreshQuestions = (worker: Worker) => {
    worker.questions = worker.questions.filter(
      question =>
        question.deadline === undefined || question.deadline > Date.now(),
    );
    worker.question = worker.questions[0];
    if (worker.state === 'asking' && !worker.question) worker.state = 'running';
    return worker.question;
  };

  const attach = (worker: Worker, child: PiProcess) => {
    worker.question = undefined;
    worker.questions = [];
    worker.process = child;
    child.onRecord(record => {
      if (worker.process !== child) return;
      onRecord(worker, record);
      inspection.record(worker.id, record);
    });
    child.onExit(() => {
      if (worker.process !== child) return; // a newer process owns this worker
      worker.process = undefined;
      worker.question = undefined;
      worker.questions = [];
      worker.awaiting = false;
      if (live(worker)) {
        worker.state = 'failed';
        worker.error = `pi exited unexpectedly; last stderr: ${child.stderr.slice(-3).join('').trim().slice(-500)}`;
        save(worker);
        notify(worker, 'failed', worker.error);
      }
    });
  };

  const onRecord = (worker: Worker, record: WorkerEvent) => {
    refreshQuestions(worker);
    switch (record.type) {
      case 'agent_start':
        worker.lastResponse = undefined;
        worker.partial = undefined;
        worker.generation++;
        worker.state = 'running';
        // Headline and phase describe one run.
        worker.headline = undefined;
        worker.headlineSource = undefined;
        worker.phase = undefined;
        worker.error = undefined;
        worker.question = undefined;
        worker.questions = [];
        worker.lastEventAt = Date.now();
        save(worker);
        return;
      case 'tool_execution_start':
        worker.tool = record.toolName;
        worker.activity = describeActivity(record.toolName, record.args);
        worker.step = describeStep(record.toolName, record.args);
        worker.calls += 1;
        worker.updatedAt = worker.lastEventAt = Date.now();
        log(worker, 'call', worker.activity);
        observe(worker);
        widget();
        return;
      case 'tool_execution_end':
        worker.tool = undefined;
        worker.failed = record.isError ? record.toolName : undefined;
        worker.lastEventAt = Date.now();
        if (record.isError) log(worker, 'result', `${record.toolName} failed`);
        observe(worker);
        widget();
        return;
      case 'message_update': {
        if (record.usage?.totalTokens) worker.tokens = record.usage.totalTokens;
        worker.lastEventAt = Date.now();
        // A finished reasoning block's heading says what the worker is working toward.
        const update = record.assistantMessageEvent;
        const heading =
          update.type === 'thinking_end' && update.content !== undefined
            ? thinkingHeading(update.content)
            : undefined;
        if (heading && worker.headlineSource !== 'progress') {
          worker.headline = label(heading, 80);
          worker.headlineSource = 'reasoning';
          widget();
        }
        return;
      }
      case 'compaction_start':
        worker.phase = 'compacting context';
        worker.lastEventAt = Date.now();
        widget();
        return;
      case 'compaction_end':
        worker.phase = undefined;
        worker.contextTokens = record.result?.estimatedTokensAfter ?? null;
        worker.lastEventAt = Date.now();
        widget();
        return;
      case 'auto_retry_start': {
        const error = record.errorMessage.split('\n', 1)[0];
        worker.phase = label(
          `retrying ${record.attempt || '?'}/${record.maxAttempts || '?'}${error ? `: ${error}` : ''}`,
          80,
        );
        worker.lastEventAt = Date.now();
        widget();
        return;
      }
      case 'auto_retry_end':
        worker.phase = undefined;
        worker.lastEventAt = Date.now();
        widget();
        return;
      case 'message_end': {
        const response = assistantResponse(record.message);
        if (response) {
          worker.lastResponse = {
            stopReason: response.stopReason,
            text: response.text.slice(0, MAX_RESULT),
            ...(response.text.length > MAX_RESULT ? {truncated: true} : {}),
            ...(response.error ? {error: response.error.slice(0, 2000)} : {}),
          };
          worker.error =
            response.stopReason === 'error'
              ? response.error || 'provider failed without an error message'
              : undefined;
          // A failed or aborted response reports no real usage; keep the last estimate.
          if (
            response.tokens &&
            response.stopReason !== 'error' &&
            response.stopReason !== 'aborted'
          )
            worker.tokens = worker.contextTokens = response.tokens;
          Object.assign(worker, addCost(worker, response));
          // A heading also arrives with the final message, for a run with no streamed thinking_end.
          for (const thinking of response.thinking) {
            const heading = thinkingHeading(thinking);
            if (heading && worker.headlineSource !== 'progress') {
              worker.headline = label(heading, 80);
              worker.headlineSource = 'reasoning';
            }
          }
        }
        const said = describeText(record.message);
        if (said) {
          worker.activity = worker.step = `\u201c${said}\u201d`;
          log(worker, 'said', said);
          observe(worker);
          widget();
        }
        worker.lastEventAt = Date.now();
        return;
      }
      case 'agent_settled': {
        worker.questions = [];
        worker.question = undefined;
        worker.phase = undefined;
        const generation = worker.generation;
        const child = worker.process;
        const response = worker.lastResponse;
        inOrder(worker, async () => {
          if (worker.process === child && worker.generation === generation)
            await settle(worker, record.aborted, response);
        }).catch((error: unknown) => {
          if (
            shuttingDown ||
            worker.process !== child ||
            worker.generation !== generation
          )
            return;
          worker.state = 'failed';
          worker.error = `settle failed: ${asError(error).message}`;
          save(worker);
          notify(worker, 'failed', worker.error);
          release(worker);
        });
        return;
      }
      case 'extension_ui_request':
        onUiRequest(worker, record);
        return;
      case 'message_start':
      case 'tool_execution_update':
        // Only the inspector's live transcript reads these.
        return;
      default:
        return record satisfies never;
    }
  };

  /** The child's extension-UI channel carries two things we care about: a
   * blocking question (dialog methods) and our own upward messages (notify). */
  const onUiRequest = (worker: Worker, record: UiRequest) => {
    const {method} = record;
    if (
      method === 'input' ||
      method === 'editor' ||
      method === 'select' ||
      method === 'confirm'
    ) {
      const text = [record.title, record.message, record.options?.join(' | ')]
        .filter(part => part)
        .join('\n');
      if (worker.questions.some(existing => existing.uiId === record.id))
        return;
      const question: Question = {
        uiId: record.id,
        text: text.slice(0, 4000) || 'question',
        method,
        ...(record.timeout === undefined
          ? {}
          : {deadline: Date.now() + Math.ceil(record.timeout)}),
      };
      worker.questions.push(question);
      worker.state = 'asking';
      refreshQuestions(worker);
      save(worker);
      if (worker.question)
        notify(
          worker,
          'asking',
          `${worker.question.text}\n\nAnswer the oldest pending question with message { to: "${worker.id}", text }.${worker.questions.length > 1 ? ` ${worker.questions.length} questions are pending.` : ''}`,
        );
      return;
    }
    if (method !== 'notify') return;
    const text = record.message ?? '';
    const usage = decodeWorkerContext(text);
    if (usage) {
      worker.model = usage.model;
      worker.contextTokens = usage.tokens;
      worker.contextWindow = usage.window;
      worker.compactAt = usage.compactAt;
      worker.compactions = usage.compactions;
      withRegistry(r => r.recordContext(worker.id, usage, Date.now()));
      save(worker);
      return;
    }
    if (!text.startsWith(MARK)) return;
    let value: unknown;
    try {
      value = JSON.parse(text.slice(MARK.length));
    } catch {
      return; // Not one of our envelopes; an ordinary notification.
    }
    const envelope = fields(value);
    const body = envelope?.text;
    const to = envelope?.to;
    if (
      typeof body !== 'string' ||
      (to !== undefined && typeof to !== 'string')
    )
      return;
    if (!to || to === 'parent') {
      note(worker, body);
      return;
    }
    // Sibling-addressed: route through this registry, visibly.
    let target: Worker;
    try {
      target = findOwned(to);
    } catch (error) {
      notify(
        worker,
        'undeliverable',
        `${asError(error).message}\n\nOriginal message:\n${body}`,
      );
      return;
    }
    pi.sendMessage(
      {
        customType: 'dots.worker.event',
        content: `Worker ${worker.label} [${worker.id}] → ${target.label} [${target.id}]\n\n${body}`,
        display: true,
        details: {id: worker.id, kind: 'relay', to: target.id},
      },
      {triggerTurn: false},
    );
    deliver(
      target,
      `Message from worker ${worker.label} [${worker.id}]:\n\n${body}`,
    ).catch((error: unknown) => {
      notify(
        worker,
        'undeliverable',
        `${asError(error).message}\n\nOriginal message:\n${body}`,
      );
    });
  };

  const settle = async (
    worker: Worker,
    aborted: boolean,
    response: AssistantResponse | undefined,
  ) => {
    if (!worker.process || worker.state === 'stopped') return;
    const generation = worker.generation;
    const child = worker.process;
    let sessionFile: string | undefined;
    try {
      sessionFile = (await child.state()).sessionFile;
    } catch (error) {
      log(
        worker,
        'note',
        `could not read the child session path: ${asError(error).message}`,
      );
    }
    // A deliver or stop that happened while we were reading owns the worker now.
    if (
      shuttingDown ||
      worker.generation !== generation ||
      worker.process !== child
    )
      return;
    worker.sessionFile = sessionFile ?? worker.sessionFile;
    const outcome = settledOutcome(response, aborted);
    const bounded =
      response?.truncated || outcome.text.length > MAX_RESULT
        ? `${outcome.text.slice(0, MAX_RESULT)}\n\n[truncated; full transcript in the session file]`
        : outcome.text;
    const spontaneous = !worker.awaiting;
    worker.state = outcome.state;
    worker.lastResponse = undefined;
    worker.error =
      outcome.state === 'failed' || outcome.state === 'unknown'
        ? bounded
        : undefined;
    worker.partial =
      outcome.state === 'interrupted' && bounded ? bounded : undefined;
    if (!spontaneous)
      worker.result = outcome.state === 'completed' ? bounded : undefined;
    worker.awaiting = false;
    worker.tool = undefined;
    worker.question = undefined;
    worker.settlements++;
    save(worker);
    const body =
      outcome.state === 'completed'
        ? bounded || '(final response had no text)'
        : outcome.state === 'interrupted'
          ? `Run interrupted; final outcome not confirmed.${bounded ? `\n\nPartial response:\n${bounded}` : ' No response text was recorded.'}`
          : bounded;
    notify(
      worker,
      spontaneous && outcome.state === 'completed' ? 'update' : outcome.state,
      body,
    );
    release(worker);
  };

  /** Starts a worker's Pi: headless, over stdio. */
  const launch = (
    worker: Worker,
    args: string[],
    childDepth: number,
  ): PiProcess =>
    new PiProcess({
      cwd: worker.cwd,
      args,
      command: options.command,
      env: workerEnv(
        {id: worker.id, depth: childDepth, root: rootId() ?? worker.id},
        process.env,
      ),
    });

  const spawn = async (
    ctx: ExtensionContext,
    input: {
      task: string;
      label: string;
      cwd?: string;
      model: string;
      thinking?: string;
      tools?: string[];
      system?: string;
      assignment?: TaskAssignment;
      skills?: string[];
    },
    signal?: AbortSignal,
  ): Promise<Worker> => {
    ensureRegistry();
    if (shuttingDown) throw new Error('worker runtime is shutting down');
    const owner = ownerSession();
    // Captured before any await: goal dispatch is judged for this runtime.
    const actor = runtimeActor();
    const config = loadConfig();
    maxWidgetLines = config.widgetLines;
    // Validate against the live scope, not the schema built at session start.
    const catalog = buildCatalog(ctx.scopedModels, ctx.model);
    const resolved = resolveChoice(
      input,
      catalog,
      {model: ctx.model, thinking: ctx.thinkingLevel},
      pi.getSettings().defaultThinkingLevel ?? 'medium',
    );
    if (!resolved.ok) throw new Error(resolved.error);
    const cwd = path.resolve(ctx.cwd, input.cwd ?? '.');
    if (!existsSync(cwd)) throw new Error(`cwd does not exist: ${cwd}`);
    let skills: SkillRef[] | undefined;
    if (input.skills?.length) {
      if (!selectable)
        throw new Error(
          'skill visibility is unknown: no run has started in this extension runtime',
        );
      skills = selectSkills(input.skills, selectable);
    }
    const assignment = input.assignment
      ? await verifyAssignment(input.assignment, cwd, actor, signal)
      : undefined;
    signal?.throwIfAborted();
    if (shuttingDown) throw new Error('worker runtime is shutting down');
    const childDepth = depth() + 1;
    if (childDepth > config.maxDepth)
      throw new Error(
        `worker nesting limit reached (maxDepth ${config.maxDepth} in workers.json)`,
      );
    const id = `w-${randomBytes(8).toString('hex')}`;
    const sessionDir = path.join(sessionsRoot(), id);
    const now = Date.now();
    const worker: Worker = {
      id,
      owner,
      label: label(input.label),
      assignment,
      skills,
      cwd,
      model: resolved.model,
      thinking: resolved.thinking,
      tools: input.tools,
      system: input.system?.trim() || undefined,
      sessionDir,
      state: 'running',
      startedAt: now,
      updatedAt: now,
      lastEventAt: now,
      tokens: 0,
      calls: 0,
      settlements: 0,
      awaiting: true,
      generation: 0,
      questions: [],
      contextWindow:
        catalog.find(entry => entry.ref === resolved.model)?.model
          .contextWindow ??
        (ctx.model && refOf(ctx.model) === resolved.model
          ? ctx.model.contextWindow
          : undefined),
    };
    // Reads the selected skill files: a failure leaves no session or record.
    const args = [
      '--session-dir',
      sessionDir,
      ...launchArgs(worker, childDepth, config.maxDepth),
    ];
    mkdirSync(sessionDir, {recursive: true});
    workers.set(id, worker);
    let child: PiProcess;
    try {
      child = launch(worker, args, childDepth);
    } catch (error) {
      worker.state = 'failed';
      worker.awaiting = false;
      worker.error = asError(error).message;
      save(worker);
      throw new Error(`${worker.id} failed to start: ${worker.error}`, {
        cause: error,
      });
    }
    attach(worker, child);
    try {
      // Know the session before the run starts, so a crash at any point after
      // the prompt leaves the worker resumable.
      worker.sessionFile = (await child.state()).sessionFile;
      const disposition = await child.prompt(input.task);
      if (shuttingDown || worker.process !== child)
        throw new Error('worker startup cancelled by stop or shutdown');
      if (disposition === 'handled')
        throw new Error('the child handled the prompt without starting a run');
    } catch (error) {
      // Never leave a ghost: record the failure and end the process.
      const diagnostic = `${asError(error).message}${child.stderr.length ? `; stderr: ${child.stderr.join('').trim().slice(-500)}` : ''}`;
      if (!shuttingDown && worker.process === child) {
        worker.process = undefined;
        worker.state = 'failed';
        worker.awaiting = false;
        worker.error = diagnostic;
        save(worker);
      }
      await child.close(1000);
      throw new Error(`${worker.id} failed to start: ${diagnostic}`, {
        cause: error,
      });
    }
    save(worker);
    return worker;
  };

  /**
   * Deliver text to a worker we own, however it is currently occupied. Every
   * delivery to an assigned worker dispatches its task again, so it repeats
   * the spawn preflight first; a refusal leaves the worker as it was.
   */
  const deliver = (
    worker: Worker,
    text: string,
    onSending?: () => void,
  ): Promise<string> => {
    if (!owned(worker)) return Promise.reject(inherited(worker));
    // Captured before any await: goal dispatch is judged for this runtime.
    const actor = runtimeActor();
    return inOrder(worker, async () => {
      if (worker.assignment) {
        const epoch = epochs.get(worker) ?? 0;
        await verifyAssignment(worker.assignment, worker.cwd, actor);
        if (shuttingDown || epoch !== (epochs.get(worker) ?? 0))
          throw new Error(
            `${worker.id} operation cancelled by stop or shutdown`,
          );
      }
      refreshQuestions(worker);
      if (!worker.process) {
        if (!worker.sessionFile)
          throw new Error(`${worker.id} has no session to continue`);
        const childDepth = depth() + 1;
        const config = loadConfig();
        if (childDepth > config.maxDepth)
          throw new Error('worker nesting limit reached');
        attach(
          worker,
          launch(
            worker,
            [
              '--session',
              worker.sessionFile,
              ...launchArgs(worker, childDepth, config.maxDepth),
            ],
            childDepth,
          ),
        );
      }
      const child = worker.process;
      if (!child) throw new Error(`${worker.id} has no active process`);
      onSending?.(); // register this delivery's waiter only after older transitions
      const question = worker.question;
      const wasRunning = worker.state === 'running';
      worker.generation++;
      worker.state = 'running';
      worker.awaiting = true;
      worker.question = undefined;
      worker.error = undefined;
      worker.lastEventAt = Date.now();
      if (!wasRunning) worker.result = undefined;
      save(worker);
      try {
        if (question) {
          child.respondUi(question.uiId, uiAnswer(question.method, text));
          worker.questions.shift();
          const nextQuestion = refreshQuestions(worker);
          if (nextQuestion) worker.state = 'asking';
          save(worker);
          if (nextQuestion)
            notify(
              worker,
              'asking',
              `${nextQuestion.text}\n\nAnswer the next pending question with message { to: "${worker.id}", text }.`,
            );
          return 'sent an answer to its question';
        }
        // prompt+steer is atomic at the host: it starts an idle child or steers
        // a busy one, including a child that settled just before this request.
        const disposition = await child.prompt(text, 'steer');
        if (shuttingDown || worker.process !== child)
          throw new Error(
            `${worker.id} delivery cancelled by stop or shutdown`,
          );
        if (disposition === 'handled')
          throw new Error(
            'the child handled the prompt without starting a run',
          );
        return disposition === 'queued'
          ? 'steered'
          : 'continued in its own session';
      } catch (error) {
        // Do not roll back a stop, shutdown, or newer owner. Failed/ambiguous
        // delivery retires this process so a retry cannot duplicate queued work.
        if (!shuttingDown && worker.process === child) {
          worker.process = undefined;
          worker.state = 'failed';
          worker.awaiting = false;
          worker.error = `delivery failed: ${asError(error).message}`;
          save(worker);
          wake(worker, 'failed', worker.error);
        }
        await child.close(1000);
        throw error;
      }
    });
  };

  type Outcome = {worker: Worker; kind: EventKind; body: string};
  /** Block until a worker (or any) settles, asks, or messages; undefined on timeout/abort. */
  const waitFor = (
    target: Worker | undefined,
    timeoutMs: number,
    signal?: AbortSignal,
  ) =>
    new Promise<Outcome | undefined>(resolve => {
      if (shuttingDown) {
        resolve(undefined);
        return;
      }
      const finish = (value: Outcome | undefined) => {
        clearTimeout(timer);
        waiters.delete(waiter);
        cancelWaiters.delete(onAbort);
        signal?.removeEventListener('abort', onAbort);
        resolve(value);
      };
      const timer = setTimeout(() => finish(undefined), timeoutMs);
      const waiter = (
        worker: Worker,
        kind: EventKind,
        body: string,
      ): boolean => {
        if (target && worker.id !== target.id) return false;
        finish({worker, kind, body});
        return true;
      };
      const onAbort = () => finish(undefined);
      waiters.add(waiter);
      cancelWaiters.add(onAbort);
      if (signal?.aborted) {
        finish(undefined);
        return;
      }
      signal?.addEventListener('abort', onAbort);
    });

  /** Stop every live worker this session owns; descendants end through each child's own shutdown. */
  const stopAll = async (): Promise<{stopped: Worker[]; closed: string[]}> => {
    const mine = [...workers.values()].filter(owned);
    const active = mine.filter(w => live(w) || (subtree(w)?.live ?? 0) > 0);
    // Close every owned process, including settled owners held for their
    // descendants, preserving finished results and states.
    const closing = mine.filter(
      w => w.process || stopping.has(w) || active.includes(w),
    );
    const closed = closing
      .filter(w => w.process || stopping.has(w))
      .map(w => w.id);
    await Promise.all(closing.map(w => stop(w, !live(w))));
    return {stopped: active, closed};
  };

  const stopAllText = ({
    stopped,
    closed,
  }: {
    stopped: Worker[];
    closed: string[];
  }): string =>
    [
      closed.length
        ? `Closed ${closed.length} owned process${closed.length === 1 ? '' : 'es'}.`
        : 'No owned processes needed closing.',
      stopped.length
        ? `Stopped active work: ${stopped.map(w => `${w.label} [${w.id}]`).join(', ')}.`
        : 'No active work remained.',
      'Finished results and saved sessions are retained; message can continue them.',
    ].join(' ');

  const stop = (worker: Worker, preserveState = false): Promise<void> => {
    if (!owned(worker)) return Promise.reject(inherited(worker));
    const existing = stopping.get(worker);
    if (existing) return existing;
    epochs.set(worker, (epochs.get(worker) ?? 0) + 1);
    worker.generation++;
    unhold(worker);
    if (!preserveState) worker.state = 'stopped';
    worker.phase = undefined;
    worker.headline = undefined;
    worker.headlineSource = undefined;
    worker.question = undefined;
    worker.questions = [];
    worker.tool = undefined;
    worker.awaiting = false;
    const child = worker.process;
    worker.process = undefined; // no late exit or settle can touch this worker now
    save(worker);
    wake(worker, 'stopped', 'stopped');
    // Close is the stop boundary, not an acknowledgement from a busy model.
    const closing = child ? child.close(2000) : Promise.resolve();
    stopping.set(worker, closing);
    const clean = () => {
      if (stopping.get(worker) === closing) stopping.delete(worker);
    };
    closing.then(clean, clean);
    return closing;
  };

  const canMessage = (worker: Worker) =>
    owned(worker) &&
    !!(worker.process || worker.sessionFile) &&
    !stopping.has(worker) &&
    !shuttingDown;
  const canStop = (worker: Worker) =>
    owned(worker) &&
    !shuttingDown &&
    !!(
      worker.process ||
      stopping.has(worker) ||
      live(worker) ||
      (subtree(worker)?.live ?? 0) > 0
    );

  const summary = (worker: Worker) => ({
    id: worker.id,
    label: worker.label,
    state: worker.state,
    owned: owned(worker),
    ...(owned(worker) ? {} : {ownerSession: worker.owner}),
    processState: stopping.has(worker)
      ? 'stopping'
      : retiring.has(worker)
        ? 'closing'
        : worker.process?.alive
          ? 'alive'
          : 'closed',
    elapsed: elapsed(worker.startedAt),
    tokens: worker.tokens,
    calls: worker.calls,
    tool: worker.tool,
    activity: worker.activity,
    lastFailure: worker.failed,
    quietFor:
      worker.state === 'running' ? elapsed(worker.lastEventAt) : undefined,
    quiet:
      worker.state === 'running' && Date.now() - worker.lastEventAt >= STALL_MS,
    children: subtree(worker),
    recent: withRegistry(r => r.events(worker.id, 8))
      ?.reverse()
      .map(e => `${e.kind}: ${e.text}`),
    question: worker.question?.text,
    pendingQuestions: worker.questions.map(question => question.text),
    cwd: worker.cwd,
    assignment: worker.assignment,
    skills: worker.skills,
    model: worker.model,
    thinking: worker.thinking,
    session: worker.sessionFile,
    headline: worker.headline,
    phase: worker.phase,
    context: contextShare(worker),
    contextWindow: worker.contextWindow,
    contextTokens: worker.contextTokens,
    compactAt: worker.compactAt,
    compactions: worker.compactions,
    cost: worker.cost,
    costBilled: worker.costBilled,
    result: worker.result,
    partial: worker.partial,
    error: worker.error,
  });

  if (selfId()) registerWorkerContext(pi);

  /**
   * Skills the model may select, from this runtime's latest run's prompt
   * options. Every run, and so every spawn, starts with before_agent_start,
   * and reload replaces this runtime.
   */
  let selectable: readonly SkillRef[] | undefined;
  pi.on('before_agent_start', (event, ctx) => {
    selectable = selectableSkills(event.systemPromptOptions.skills);
    track(ctx);
  });

  /**
   * Rebuilds workers from every entry of this session, in file order (the
   * latest record of a worker wins), so /tree navigation and reload agree.
   * An invalid record is diagnosed and skipped, and its worker is not
   * resurrected from an older record.
   */
  const restore = (entries: readonly SessionEntry[]) => {
    const report = new RestoreReport();
    const latest = new Map<string, WorkerRecord | undefined>();
    for (const entry of entries) {
      if (entry.type !== 'custom' || entry.customType !== WORKER_ENTRY)
        continue;
      const decoded = decode(WorkerRecordSchema, entry.data);
      if (decoded.ok) {
        latest.set(decoded.value.id, decoded.value);
        continue;
      }
      report.add(decoded.error);
      const id = fields(entry.data)?.id;
      if (typeof id === 'string') latest.set(id, undefined);
    }
    for (const [id, record] of latest) {
      const existing = workers.get(id);
      if (existing?.process || !record) continue;
      const worker = restored(record);
      // Children died with the previous process; their sessions persist and can be continued.
      if (live(worker)) worker.state = 'paused';
      workers.set(id, worker);
    }
    return report.message('worker');
  };

  pi.on('session_start', async (_event, ctx) => {
    // First: a worker's only channel to its parent must not depend on later setup.
    registerMessaging(ctx.mode !== 'tui');
    track(ctx);
    sessionId = ctx.sessionManager.getSessionId();
    refreshSpawn(ctx);
    try {
      maxWidgetLines = loadConfig().widgetLines;
    } catch (error) {
      warn(asError(error).message);
    }
    const problem = restore(ctx.sessionManager.getEntries());
    if (problem) warn(problem);
    ensureRegistry();
    if (shuttingDown) return;
    for (const worker of workers.values()) observe(worker);
    widget();
    // One root session prunes for the whole tree. Without the registry a
    // worker's owner is unknown, so nothing is pruned.
    if (selfId() === undefined && registry)
      pruneSessions(
        sessionsRoot(),
        id =>
          !!workers.get(id)?.process ||
          withRegistry(r => r.ownerAlive(id)) !== false,
      ).then(
        removed => {
          for (const id of removed) withRegistry(r => r.forget(id));
        },
        (error: unknown) =>
          warn(
            `old worker sessions could not be pruned: ${asError(error).message}`,
          ),
      );
  });

  pi.on('session_shutdown', async () => {
    // Stop UI callbacks before awaiting children: reload invalidates this ctx.
    if (shuttingDown) return;
    shuttingDown = true;
    delivery.close();
    animate(false);
    publishActivity(pi.events, 'workers', []);
    widgetRows = [];
    publishWorkerActivity();
    inspection.dispose();
    checkoutResolver.dispose();
    if (ticker) clearInterval(ticker);
    ticker = undefined;
    requestRender = undefined;
    widgetMounted = false;
    for (const cancel of [...cancelWaiters]) cancel();
    const closing: Promise<unknown>[] = [];
    held.clear();
    drained.clear();
    for (const worker of workers.values()) {
      const pending = stopping.get(worker) ?? retiring.get(worker);
      if (pending) closing.push(pending);
    }
    for (const worker of workers.values()) {
      const child = worker.process;
      if (!child) continue;
      worker.process = undefined;
      worker.state = live(worker) ? 'paused' : worker.state;
      pi.appendEntry(WORKER_ENTRY, persisted(worker));
      observe(worker);
      closing.push(child.close(3000));
    }
    context = undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    await Promise.race([
      Promise.all(closing),
      new Promise<void>(resolve => {
        timer = setTimeout(resolve, SHUTDOWN_MS);
      }),
    ]);
    clearTimeout(timer);
    withRegistry(r => {
      r.close();
    });
    registry = undefined;
  });

  /** The spawn schema lists the session's real models, so it is rebuilt when the scope or current model changes. */
  const registerSpawn = (
    catalog: readonly CatalogEntry[],
    ctx?: ExtensionContext,
  ) => {
    const parent = {model: ctx?.model, thinking: ctx?.thinkingLevel};
    pi.registerTool({
      name: 'worker_spawn',
      label: 'Spawn worker',
      description: [
        'Start a headless Pi child with its own context and session. Returns immediately with an id; its final message arrives later as a worker event that starts your next turn. Give it a self-contained brief: target (files, non-goals), change, acceptance, checks, stopping point. Use cwd for a worktree. tools is an allowlist; omit for the defaults.',
        CHOICE_GUIDANCE,
        describeCatalog(catalog, parent),
      ].join('\n\n'),
      promptSnippet:
        'Delegate a bounded piece of work to a background child; results come back as events.',
      parameters: Type.Object({
        label: Type.String({
          description: 'Short task-named label, e.g. netpol-api',
        }),
        task: Type.String({description: 'The complete brief'}),
        model: Type.Union(
          [Type.Literal('inherit'), ...catalog.map(e => Type.Literal(e.ref))],
          {description: 'inherit, or a provider/id from the Models list'},
        ),
        thinking: Type.Optional(
          Type.Union(
            [
              Type.Literal('default'),
              Type.Literal('inherit'),
              ...THINKING_LEVELS.map(level => Type.Literal(level)),
            ],
            {
              description:
                'Omit for default; otherwise inherit or a level the chosen model supports',
            },
          ),
        ),
        cwd: Type.Optional(
          Type.String({
            description:
              "Working directory selected using the shared worktrees guidance; relative to this session's cwd",
          }),
        ),
        assignment: Type.Optional(
          Type.Object(
            {
              project: Type.String({
                minLength: 1,
                maxLength: 4096,
                description:
                  'Exact project key returned by dots task; not inferred from cwd',
              }),
              id: Type.String({
                pattern: '^t-[a-f0-9]+$',
                description:
                  'Existing task ID; several workers may contribute to one task',
              }),
            },
            {
              description:
                'Optional task association, checked before launch and retained on resume. Does not claim or update tasks.',
            },
          ),
        ),
        tools: Type.Optional(
          Type.Array(Type.String({minLength: 1, maxLength: 256}), {
            maxItems: 256,
            description:
              'Tool allowlist, e.g. [read, grep, find, ls] for read-only; message is always included',
          }),
        ),
        system: Type.Optional(
          Type.String({
            description:
              'Extra system prompt for this child; the child contract is always appended',
          }),
        ),
        skills: Type.Optional(
          Type.Array(Type.String({minLength: 1, maxLength: 256}), {
            maxItems: 64,
            description:
              'Names of your available skills material to this brief; the child starts with each SKILL.md loaded in full. Files a skill references stay on disk for the child to read. None are passed by default.',
          }),
        ),
      }),
      async execute(_id, params, signal, _update, ctx) {
        track(ctx);
        const worker = await spawn(ctx, params, signal);
        return {
          content: [
            {
              type: 'text',
              text: `Spawned ${worker.id} (${worker.label}) in ${worker.cwd} on ${worker.model} · thinking ${worker.thinking}${worker.skills ? ` · skills ${worker.skills.map(skill => skill.name).join(', ')}` : ''}. Its result will arrive as a worker event; continue with other work.`,
            },
          ],
          details: summary(worker),
        };
      },
    });
  };
  const refreshSpawn = (ctx: ExtensionContext) =>
    registerSpawn(buildCatalog(ctx.scopedModels, ctx.model), ctx);
  registerSpawn([]);
  pi.on('model_select', (_event, ctx) => refreshSpawn(ctx));

  /** Upward messages ride Pi's RPC extension-UI notify to the owner. */
  const tellParent = (ctx: ExtensionContext, envelope: string) => {
    ctx.ui.notify(envelope, 'info');
  };

  const sendMessage = async (
    params: {to: string; text: string; wait?: boolean; timeoutMs?: number},
    signal: AbortSignal | undefined,
    ctx: ExtensionContext,
  ): Promise<AgentToolResult<unknown>> => {
    track(ctx);
    const me = selfId();
    if (params.to === 'parent') {
      if (!me) throw new Error('this session has no parent');
      if (params.wait) {
        if (parentQuestionPending)
          throw new Error(
            'A parent question is already awaiting an answer; combine questions or wait for it first',
          );
        parentQuestionPending = true;
        try {
          const timeout = params.timeoutMs ?? 600_000;
          const answer = await ctx.ui.input(params.text, undefined, {
            timeout,
            ...(signal ? {signal} : {}),
          });
          return {
            content: [
              {
                type: 'text',
                text:
                  answer === undefined
                    ? 'The parent did not answer (cancelled or timed out). No decision was approved; report the blocker or ask again.'
                    : answer,
              },
            ],
            details: {answered: answer !== undefined},
          };
        } finally {
          parentQuestionPending = false;
        }
      }
      tellParent(
        ctx,
        `${MARK}${JSON.stringify({to: 'parent', text: params.text})}`,
      );
      return {
        content: [{type: 'text', text: 'progress note sent to parent'}],
        details: {to: 'parent'},
      };
    }
    let worker: Worker | undefined;
    try {
      worker = find(params.to);
    } catch (error) {
      if (!me) throw error;
      // The relay has no correlated reply channel; do not pretend to wait.
      if (params.wait)
        throw new Error(
          'wait: true is not supported for sibling relays; ask the parent to coordinate the reply',
        );
      // Not ours: a sibling. The owner routes it through its registry.
      tellParent(
        ctx,
        `${MARK}${JSON.stringify({to: params.to, text: params.text})}`,
      );
      return {
        content: [{type: 'text', text: `sent to ${params.to} via the parent`}],
        details: {to: params.to, relayed: true},
      };
    }
    if (!owned(worker)) throw inherited(worker);
    const target = worker;
    const cancellation = new AbortController();
    const waitSignal = signal
      ? AbortSignal.any([signal, cancellation.signal])
      : cancellation.signal;
    const waiting: {pending?: Promise<Outcome | undefined>} = {};
    let disposition: string;
    try {
      disposition = await deliver(target, params.text, () => {
        if (params.wait)
          waiting.pending = waitFor(
            target,
            params.timeoutMs ?? 600_000,
            waitSignal,
          );
      });
    } catch (error) {
      cancellation.abort();
      throw error;
    }
    if (!waiting.pending) {
      return {
        content: [{type: 'text', text: `${target.id}: ${disposition}`}],
        details: summary(target),
      };
    }
    const outcome = await waiting.pending;
    if (!outcome)
      return {
        content: [
          {
            type: 'text',
            text: `${target.id}: ${disposition}; no reply within the wait window (still ${target.state})`,
          },
        ],
        details: summary(target),
      };
    return {
      content: [
        {
          type: 'text',
          text: `${target.id} ${outcome.kind}\n\n${outcome.body}`,
        },
      ],
      details: summary(target),
    };
  };

  const messageDescription = (blocking: boolean) =>
    `Send text to another session: a worker you spawned (by id or label), a sibling worker (by id; the owner relays it), or "parent" when you are a worker. To the parent${blocking ? ' without wait' : ''}, it is a progress note: shown as your status and added to the parent's context without interrupting it. Delivery adapts to the target: it answers a worker that is blocked on a question, steers a running one between tool calls, or continues a finished, stopped, or paused one in its own session.`;

  /**
   * Blocking waits exist only outside the interactive TUI: there, worker
   * events start the next turn, and a blocked tool call would hold the user's
   * input until cancelled. Pi has no tool unregistration, and the mode is known
   * only from a context, so these tools are registered at session start.
   */
  const registerMessaging = (blocking: boolean) => {
    if (!blocking) {
      pi.registerTool({
        name: 'message',
        label: 'Message',
        description: messageDescription(false),
        promptSnippet: 'Talk to a worker or your parent.',
        parameters: Type.Object({
          to: Type.String({description: 'Worker id or label, or "parent"'}),
          text: Type.String(),
        }),
        execute: (_id, params, signal, _update, ctx) =>
          sendMessage(params, signal, ctx),
      });
      return;
    }
    pi.registerTool({
      name: 'message',
      label: 'Message',
      description: `${messageDescription(true)} wait: true blocks until an owned worker replies or settles, or the parent answers; sibling relays do not support waiting. Use it for questions that need an answer before you can proceed.`,
      promptSnippet:
        'Talk to a worker or your parent; wait: true to block for the reply.',
      parameters: Type.Object({
        to: Type.String({description: 'Worker id or label, or "parent"'}),
        text: Type.String(),
        wait: Type.Optional(
          Type.Boolean({
            description:
              'Block until the target replies or settles (default false)',
          }),
        ),
        timeoutMs: Type.Optional(
          Type.Integer({
            minimum: 1,
            maximum: MAX_WAIT_MS,
            description: 'With wait: give up after this long (default 600000)',
          }),
        ),
      }),
      execute: (_id, params, signal, _update, ctx) =>
        sendMessage(params, signal, ctx),
    });

    pi.registerTool({
      name: 'worker_wait',
      label: 'Wait for worker',
      description:
        "Block until a worker settles, asks a question, or sends a message. id: that worker; no id: the first of this session's workers to do so (call again for the next); all: true: until every live worker has settled, returning each result. Use when you have nothing else to do.",
      parameters: Type.Object({
        id: Type.Optional(Type.String()),
        all: Type.Optional(
          Type.Boolean({
            description:
              'Wait for every live worker of this session; a question from any of them returns early so it can be answered',
          }),
        ),
        timeoutMs: Type.Optional(
          Type.Integer({
            minimum: 1,
            maximum: MAX_WAIT_MS,
            description:
              'Give up after this long (default 600000); the worker keeps running',
          }),
        ),
      }),
      async execute(_id, params, signal, _update, ctx) {
        track(ctx);
        const running = (w: Worker) => w.state === 'running';
        const mine = () => [...workers.values()].filter(owned);
        if (params.all) {
          const asking = mine().find(w => w.state === 'asking');
          if (asking)
            return {
              content: [
                {
                  type: 'text',
                  text: `${asking.id} asking (answer it, then wait again)\n\n${asking.question?.text ?? ''}`,
                },
              ],
              details: summary(asking),
            };
          const deadline = Date.now() + (params.timeoutMs ?? 600_000);
          const seen: {
            text: string;
            details: ReturnType<typeof summary>;
            key: string;
          }[] = [];
          const include = (
            worker: Worker,
            kind: string,
            body: string,
            key: string,
          ) => {
            if (seen.some(item => item.key === key)) return;
            seen.push({
              text: `${worker.id} ${worker.label} ${kind}\n\n${body}`.trimEnd(),
              details: summary(worker),
              key,
            });
          };
          // An already-settled result is consumed only if this tool returns it.
          for (const worker of mine()) {
            const key = `${worker.id}:${worker.settlements}`;
            const queued = running(worker) ? undefined : outbox.queued(key);
            if (!queued) continue;
            const body = queued.content.split('\n\n').slice(1).join('\n\n');
            include(
              worker,
              queued.details.kind,
              queued.details.kind === 'completed'
                ? (worker.result ?? body)
                : body,
              key,
            );
          }
          const collected = () => seen.map(item => item.text);
          const consumeSeen = () => {
            for (const item of seen) outbox.consume(item.key);
          };
          while (mine().some(running)) {
            const outcome = await waitFor(
              undefined,
              Math.max(1, deadline - Date.now()),
              signal,
            );
            if (!outcome) break;
            if (outcome.kind === 'asking' || outcome.kind === 'message') {
              consumeSeen();
              const text = [
                ...collected(),
                `${outcome.worker.id} ${outcome.kind} (answer it, then wait again)\n\n${outcome.body}`,
              ].join('\n\n---\n\n');
              return {
                content: [{type: 'text', text}],
                details: seen.length
                  ? [...seen.map(item => item.details), summary(outcome.worker)]
                  : summary(outcome.worker),
              };
            }
            include(
              outcome.worker,
              outcome.kind,
              outcome.body,
              `${outcome.worker.id}:${outcome.worker.settlements}`,
            );
          }
          const now = Date.now();
          const still = mine().filter(running);
          consumeSeen();
          const text = [
            ...collected(),
            still.length
              ? `Still running at the wait window:\n${still.map(w => describe(w, now, false, 0)).join('\n')}`
              : '',
          ]
            .filter(Boolean)
            .join('\n\n---\n\n');
          return {
            content: [{type: 'text', text: text || 'No live workers.'}],
            details: [...seen.map(item => item.details), ...still.map(summary)],
          };
        }
        const target = params.id ? findOwned(params.id) : undefined;
        const candidates = target ? [target] : mine();
        if (candidates.length === 0)
          return {
            content: [{type: 'text', text: 'No workers in this session.'}],
            details: [],
          };
        const already = candidates.find(w => !running(w));
        if (already && (target || candidates.every(w => !running(w)))) {
          outbox.consume(`${already.id}:${already.settlements}`);
          return {
            content: [
              {
                type: 'text',
                text: `${already.id} is ${already.state}${already.result ? `\n\n${already.result}` : ''}${already.question ? `\n\nasking: ${already.question.text}` : ''}`,
              },
            ],
            details: summary(already),
          };
        }
        const outcome = await waitFor(
          target,
          params.timeoutMs ?? 600_000,
          signal,
        );
        if (!outcome) {
          const now = Date.now();
          const rows = candidates
            .filter(running)
            .map(w => describe(w, now, false, 0));
          return {
            content: [
              {
                type: 'text',
                text: `No change within the wait window; still running:\n${rows.join('\n')}`,
              },
            ],
            details: candidates.map(summary),
          };
        }
        return {
          content: [
            {
              type: 'text',
              text: `${outcome.worker.id} ${outcome.kind}\n\n${outcome.body}`,
            },
          ],
          details: summary(outcome.worker),
        };
      },
    });
  };

  /**
   * The model-facing line for a worker: what it is, how long, and what it is
   * doing right now (or why it is not), with descendants from the registry.
   */
  const describeLine = (w: Worker, now: number): string => {
    const since = now - w.lastEventAt;
    const parts = [
      `${w.state} · age ${duration(now - w.startedAt)}`,
      `${compact(w.tokens)} tok`,
      `${w.calls} call${w.calls === 1 ? '' : 's'}`,
    ];
    parts.push(contextLabel(w));
    if (w.cost !== undefined) parts.push(costDetail(w.cost, w.costBilled));
    if (!owned(w))
      parts.push(`read-only: owned by session ${label(w.owner, 80)}`);
    if (w.phase) parts.push(w.phase);
    else if (w.headline && live(w)) parts.push(`toward: ${w.headline}`);
    if (w.state === 'running') {
      parts.push(
        w.tool
          ? `now: ${label(w.activity ?? w.tool, 80)} (${duration(since)} in)`
          : w.activity
            ? `last: ${label(w.activity, 80)} (${duration(since)} ago)`
            : `no events yet (${duration(since)})`,
      );
      if (since >= STALL_MS) parts.push(`QUIET ${duration(since)}`);
      if (w.failed) parts.push(`last call failed: ${w.failed}`);
    }
    if (w.state === 'asking')
      parts.push(
        `asking (${duration(since)} ago): ${label(w.question?.text ?? '', 200)}`,
      );
    // A settled worker's process is closed unless it is still exiting or held.
    if (!live(w) && owned(w)) {
      if (stopping.has(w)) parts.push('process stopping');
      else if (retiring.has(w)) parts.push('process closing');
      else if (w.process) parts.push('process kept for running descendants');
    }
    if (w.assignment)
      parts.push(
        `task ${w.assignment.id} in ${label(w.assignment.project, 256)}`,
      );
    if (w.model)
      parts.push(`${w.model}${w.thinking ? ` · thinking ${w.thinking}` : ''}`);
    return `${w.id}  ${w.label} \u00b7 ${parts.join(' \u00b7 ')}`;
  };
  const describeDescendants = (
    parent: string,
    level: number,
    now: number,
    seen = new Set<string>(),
  ): string[] => {
    if (seen.has(parent)) return [];
    seen.add(parent);
    return (withRegistry(r => r.children(parent)) ?? []).flatMap(r => {
      if (seen.has(r.id)) return [];
      const since = now - r.last_event_at;
      const doing =
        r.state === 'running'
          ? r.activity
            ? ` \u00b7 ${label(r.activity, 70)} (${duration(since)} ago)`
            : ''
          : r.state === 'asking'
            ? ` \u00b7 asking: ${label(r.question ?? '', 120)}`
            : '';
      const line = `${'  '.repeat(level)}\u2937 ${r.id}  ${r.label} \u00b7 ${r.state} · age ${duration(now - r.started_at)} \u00b7 ${compact(r.tokens)} tok · ${descendantContext(r.id, now, r.state === 'lost').text}${doing}`;
      return [line, ...describeDescendants(r.id, level + 1, now, seen)];
    });
  };
  const describe = (
    w: Worker,
    now: number,
    detail: boolean,
    tailLines: number,
  ): string => {
    let out = describeLine(w, now);
    const kids = owned(w) ? describeDescendants(w.id, 1, now) : [];
    if (kids.length) out += `\n${kids.join('\n')}`;
    if (!detail && w.result)
      out += `\n  result: ${label(w.result.split('\n').find(line => line.trim()) ?? '', 120)}`;
    if (w.error) out += `\n  error: ${w.error}`;
    if (w.partial)
      out += `\n  partial response (not final): ${label(w.partial.split('\n').find(line => line.trim()) ?? '', 120)}`;
    if (w.sessionFile) out += `\n  session: ${w.sessionFile}`;
    if (detail) {
      out += `\n  checkout: ${w.cwd}`;
      if (w.assignment)
        out += `\n  assignment: ${JSON.stringify(w.assignment)}`;
      for (const skill of w.skills ?? [])
        out += `\n  skill: ${label(skill.name, 256)} ${label(skill.path, 4096)}`;
      const trail = owned(w)
        ? (withRegistry(r => r.events(w.id, 30))?.reverse() ?? [])
        : [];
      if (trail.length)
        out += `\n\nactivity (oldest first):\n${trail.map(e => `  ${new Date(e.at).toISOString().slice(11, 19)} ${e.kind.padEnd(6)} ${label(e.text, 120)}`).join('\n')}`;
      const tail = transcriptTail(w.sessionFile, tailLines);
      if (tail.length) out += `\n\ntranscript tail:\n${tail.join('\n')}`;
      if (w.result) out += `\n\nresult:\n${w.result}`;
      if (w.partial) out += `\n\npartial response (not final):\n${w.partial}`;
    }
    return out;
  };
  /** The nearest ancestor of a registry row that this session owns, if any. */
  const ownedAncestor = (
    parentId: string | null,
  ): {owner: string; depth: number} | undefined => {
    const seen = new Set<string>();
    let parent = parentId;
    while (parent && !seen.has(parent)) {
      const own = workers.get(parent);
      if (own)
        return owned(own) ? {owner: parent, depth: seen.size + 1} : undefined;
      seen.add(parent);
      const id: string = parent;
      parent = withRegistry(registry => registry.get(id))?.parent ?? null;
    }
    return undefined;
  };
  /** A descendant we do not own: read-only view from the registry. */
  const describeForeign = (
    id: string,
    now: number,
    tailLines: number,
  ): string | undefined => {
    const r = withRegistry(registry => {
      registry.sweep();
      return registry.get(id);
    });
    if (!r || !ownedAncestor(r.parent)) return undefined;
    const tail = transcriptTail(r.session_file ?? undefined, tailLines);
    const trail =
      withRegistry(registry => registry.events(r.id, 30))?.reverse() ?? [];
    return [
      `${r.id}  ${r.label} \u00b7 ${r.state} · age ${duration(now - r.started_at)} \u00b7 ${compact(r.tokens)} tok \u00b7 ${r.calls} calls${r.activity ? ` \u00b7 last: ${label(r.activity, 80)} (${duration(now - r.last_event_at)} ago)` : ''}`,
      `  ${descendantContext(r.id, now, r.state === 'lost').text}`,
      `  owned by ${r.parent ?? 'another session'}; to steer or stop it, message its owner.`,
      r.session_file ? `  session: ${r.session_file}` : '',
      `  checkout: ${r.cwd}`,
      r.task_id ? `  task: ${r.task_id} in ${r.task_project}` : '',
      r.question ? `  asking: ${r.question}` : '',
      r.failed ? `  last call failed: ${r.failed}` : '',
      tail.length ? `\ntranscript tail:\n${tail.join('\n')}` : '',
      '  final result is owned by its parent; inspect the session transcript for complete history.',
      trail.length
        ? `\nactivity (oldest first):\n${trail.map(e => `  ${new Date(e.at).toISOString().slice(11, 19)} ${e.kind.padEnd(6)} ${label(e.text, 120)}`).join('\n')}`
        : '',
    ]
      .filter(Boolean)
      .join('\n');
  };

  /**
   * Default listings: live and paused workers, workers with a process, and
   * those finished within LISTED_MS. Lookup by id or label finds every worker.
   */
  const listed = (worker: Worker, now: number) =>
    live(worker) ||
    worker.state === 'paused' ||
    !!worker.process ||
    stopping.has(worker) ||
    retiring.has(worker) ||
    (worker.finishedAt !== undefined && now - worker.finishedAt < LISTED_MS);
  const listing = (all: boolean) => {
    const now = Date.now();
    const every = [...workers.values()];
    const shown = all ? every : every.filter(w => listed(w, now));
    const older = every.length - shown.length;
    return {
      shown,
      older:
        older > 0
          ? `${older} older worker${older === 1 ? '' : 's'}; worker_status { all: true } or /workers all`
          : undefined,
    };
  };

  pi.registerTool({
    name: 'worker_status',
    label: 'Worker status',
    description:
      "List this session's workers and their live descendants (what each is doing right now, how long since its last event, QUIET when stalled, last failed call, pending question), or inspect one by id: its activity trail, transcript tail, and full result. The list shows live and paused workers and those finished within the last hour; all: true lists every worker, and id finds any of them. A descendant's id shows read-only registry activity and transcript tail; its final result belongs to its owner. Message its owner to steer it. Completions arrive as events; use this to look inside, not to wait.",
    parameters: Type.Object({
      id: Type.Optional(Type.String()),
      all: Type.Optional(
        Type.Boolean({
          description:
            'List every worker this session has had, not only live and recent ones',
        }),
      ),
      tail: Type.Optional(
        Type.Integer({
          minimum: 0,
          maximum: 100,
          description:
            'Recent transcript lines to include for one worker (default 20)',
        }),
      ),
    }),
    async execute(_id, params, _signal, _update, ctx) {
      track(ctx);
      ensureRegistry();
      if (shuttingDown) throw new Error('worker runtime is shutting down');
      const now = Date.now();
      const wanted = params.id?.trim();
      if (wanted) {
        let own: Worker | undefined;
        try {
          own = find(wanted);
        } catch (error) {
          const foreign = describeForeign(wanted, now, params.tail ?? 20);
          if (foreign)
            return {
              content: [{type: 'text', text: foreign}],
              details: withRegistry(r => ({
                ...r.get(wanted),
                contextObservation: r.context(wanted),
              })),
            };
          throw error;
        }
        return {
          content: [
            {type: 'text', text: describe(own, now, true, params.tail ?? 20)},
          ],
          details: summary(own),
        };
      }
      const {shown, older} = listing(params.all ?? false);
      const lines = shown.map(w => describe(w, now, false, 0));
      if (older) lines.push(older);
      const text = lines.length
        ? lines.join('\n')
        : 'No workers in this session.';
      return {content: [{type: 'text', text}], details: shown.map(summary)};
    },
  });

  pi.registerTool({
    name: 'worker_stop',
    label: 'Stop worker',
    description:
      "Abort a running or asking worker and end its process (and, through its own shutdown, its descendants). A finished worker's process exits on its own once its descendants settle; stopping one that still waits on descendants ends them and keeps its result. Its session file stays; message can continue it later. all: true closes every owned process and stops all active work, preserving finished results.",
    parameters: Type.Object({
      id: Type.Optional(Type.String()),
      all: Type.Optional(Type.Boolean()),
    }),
    async execute(_id, params, _signal, _update, ctx) {
      track(ctx);
      if (params.all) {
        const outcome = await stopAll();
        return {
          content: [{type: 'text', text: stopAllText(outcome)}],
          details: {
            stopped: outcome.stopped.map(summary),
            closed: outcome.closed,
          },
        };
      }
      if (!params.id) throw new Error('worker_stop needs id or all: true');
      const worker = findOwned(params.id);
      const resumable = worker.sessionFile
        ? `Saved session retained at ${worker.sessionFile}; message can continue it.`
        : 'No saved session is available to resume.';
      if (!canStop(worker))
        return {
          content: [
            {
              type: 'text',
              text: `${worker.id} is already ${worker.state} and has no process; nothing to stop. ${resumable}`,
            },
          ],
          details: summary(worker),
        };
      const active = live(worker);
      await stop(worker, !active);
      return {
        content: [
          {
            type: 'text',
            text: `${worker.id} ${active ? 'stopped' : `process closed; it stays ${worker.state}`}. ${resumable}`,
          },
        ],
        details: summary(worker),
      };
    },
  });

  const listItem = (worker: Worker, level = 0): WorkerListItem => ({
    id: worker.id,
    label: worker.label,
    state: worker.state,
    depth: level,
    canMessage: canMessage(worker),
    canStop: canStop(worker),
    ...(owned(worker) ? {} : {owner: `session ${worker.owner}`}),
  });

  /** Listed workers, each followed by its descendants from the registry. */
  const inspectionList = (all: boolean): WorkerListItem[] => {
    const seen = new Set<string>();
    const rows: WorkerListItem[] = [];
    const own = listing(all).shown;
    const descendants = (
      parent: string,
      level: number,
      limit: number,
    ): void => {
      for (const child of withRegistry(r => r.children(parent)) ?? []) {
        if (rows.length >= limit) return;
        if (seen.has(child.id)) continue;
        seen.add(child.id);
        rows.push({
          id: child.id,
          label: child.label,
          state: child.state,
          depth: level,
          owner: child.parent ?? parent,
          canMessage: false,
          canStop: false,
        });
        descendants(child.id, level + 1, limit);
      }
    };
    for (const [index, worker] of own.entries()) {
      if (rows.length >= 201) break;
      seen.add(worker.id);
      rows.push(listItem(worker));
      // Reserve room for remaining direct owners before filling their trees.
      if (owned(worker))
        descendants(
          worker.id,
          1,
          Math.max(rows.length, 201 - (own.length - index - 1)),
        );
    }
    return rows;
  };
  const inspectionNotice = (all: boolean): string | undefined =>
    [
      registry ? undefined : 'Descendant observations unavailable',
      listing(all).older,
    ]
      .filter(Boolean)
      .join(' · ') || undefined;

  const inspectionSource: WorkerInspectionSource = {
    notice: () => inspectionNotice(false),
    list: () => inspectionList(false),
    read: id => {
      const own = workers.get(id);
      if (own) {
        const git = checkoutResolver.branch(own.cwd);
        return {
          worker: listItem(own),
          detail: `${id} · ${own.state} · ${own.calls} calls · ${own.tokens} tok\nContext: ${contextLabel(own)} · Cost: ${own.cost === undefined ? 'unknown' : costDetail(own.cost, own.costBilled)}\nDirectory: ${own.cwd}${git?.kind === 'branch' ? `\nGit ${git.detached ? 'HEAD' : 'branch'}: ${git.label}` : ''}${own.skills ? `\nSkills: ${own.skills.map(skill => `${skill.name} (${skill.path})`).join(', ')}` : ''}${own.error ? `\nError: ${own.error}` : ''}${own.partial ? `\nPartial response (not final): ${own.partial}` : ''}${own.result ? `\nLast retained final text: ${own.result}` : own.state === 'completed' ? '\nFinal response had no text.' : ''}`,
          ...(own.sessionFile === undefined
            ? {}
            : {sessionFile: own.sessionFile}),
        };
      }
      const foreign = withRegistry(r => r.get(id));
      if (!foreign) return undefined;
      const ancestor = ownedAncestor(foreign.parent);
      if (!ancestor) return undefined;
      const git = checkoutResolver.branch(foreign.cwd);
      const worker: WorkerListItem = {
        id,
        label: foreign.label,
        state: foreign.state,
        depth: ancestor.depth,
        owner: foreign.parent ?? ancestor.owner,
        canMessage: false,
        canStop: false,
      };
      return {
        worker,
        detail: `${id} · ${foreign.state} · ${foreign.calls} calls · ${foreign.tokens} tok\nContext: ${descendantContext(id, Date.now(), foreign.state === 'lost').text}\nDirectory: ${foreign.cwd}${git?.kind === 'branch' ? `\nGit ${git.detached ? 'HEAD' : 'branch'}: ${git.label}` : ''}${foreign.question ? `\nAsking: ${foreign.question}` : ''}${foreign.failed ? `\nLast call failed: ${foreign.failed}` : ''}`,
        ...(foreign.session_file === null
          ? {}
          : {sessionFile: foreign.session_file}),
      };
    },
    ...(process.env.REX_SESSION
      ? {
          openInRex: async (id: string) => {
            const snapshot = inspectionSource.read(id);
            if (!snapshot) throw new Error('Worker is no longer available');
            if (!snapshot.sessionFile)
              throw new Error('Worker has no saved transcript yet');
            await viewInRex(pi, snapshot.sessionFile, snapshot.worker.label);
          },
        }
      : {}),
    message: async (id, text) => {
      const worker = workers.get(id);
      if (!worker || !canMessage(worker))
        throw worker && !owned(worker)
          ? inherited(worker)
          : new Error('Worker is not owned or has no session to continue');
      const disposition = await deliver(worker, text);
      if (!shuttingDown)
        pi.sendMessage(
          {
            customType: 'dots.worker.event',
            content: `You sent a message to ${worker.label} [${id}] from the worker inspector (${disposition}).`,
            display: true,
            details: {
              id,
              kind: 'message',
              label: worker.label,
            } satisfies EventDetails,
          },
          {triggerTurn: false, deliverAs: 'steer'},
        );
    },
    stop: async id => {
      const worker = workers.get(id);
      if (!worker || !canStop(worker))
        throw worker && !owned(worker)
          ? inherited(worker)
          : new Error('Worker is not owned or active here');
      await stop(worker, !live(worker));
      if (!shuttingDown)
        pi.sendMessage(
          {
            customType: 'dots.worker.event',
            content: `You stopped ${worker.label} [${id}] from the worker inspector.`,
            display: true,
            details: {
              id,
              kind: 'stopped',
              label: worker.label,
            } satisfies EventDetails,
          },
          {triggerTurn: false, deliverAs: 'steer'},
        );
    },
  };

  pi.registerCommand('workers', {
    description:
      "Inspect this session's live and recent workers and their descendants; `/workers all` lists every worker; `/workers <id>` opens one directly; `/workers stop` stops all",
    handler: async (args, ctx) => {
      if (!ctx.hasUI || shuttingDown) return;
      const wanted = args.trim();
      if (wanted === 'stop' || wanted === 'stop-all') {
        const outcome = await stopAll();
        if (!shuttingDown) ctx.ui.notify(stopAllText(outcome), 'info');
        return;
      }
      track(ctx);
      ensureRegistry();
      if (shuttingDown) return;
      const all = wanted === 'all';
      const id =
        wanted && !all
          ? (() => {
              try {
                return find(wanted).id;
              } catch (error) {
                if (inspectionSource.read(wanted)) return wanted;
                throw error;
              }
            })()
          : undefined;
      await inspection.open(
        ctx,
        all
          ? {
              ...inspectionSource,
              list: () => inspectionList(true),
              notice: () => inspectionNotice(true),
            }
          : inspectionSource,
        transcriptOptions(pi.getSettings()),
        id,
      );
    },
  });
}
