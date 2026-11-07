import path from 'node:path';
import type {ExtensionAPI} from '@earendil-works/pi-coding-agent';
import {dots, type DotsOptions} from '@joshrwolf/pi-kit/dots';
import {watchChanges} from '../../src/host/changes.ts';

const INDEX = 'MEMORY.md';
const MEMORY_SECTION_PREFIX = 'dots_memory_';
type Layer = 'global' | 'project';
interface Index {
  readonly scope: string;
  readonly snapshot: string;
  readonly state: 'available';
  readonly exists: boolean;
  readonly content: string;
  readonly nextLine: number | null;
}
interface Unavailable {
  readonly scope: string;
  readonly snapshot: string;
  readonly state: 'unavailable';
  readonly error: string;
}
/** How current this machine's local copy is; the sync agent keeps it so. */
interface Freshness {
  /** Unix seconds when the copy was last confirmed current. */
  readonly asOf: number;
  /** An older copy means the sync agent has missed several passes. */
  readonly staleAfter: number;
  /** Set while sync passes fail, such as when the remote is unreachable. */
  readonly failing: {readonly since: number; readonly lastError: string} | null;
}
interface MemorySnapshot {
  readonly store: string;
  readonly snapshot: string;
  readonly freshness: Freshness;
  /** Unpublished candidates retained on this machine. */
  readonly pending: number;
  /** Marker file touched whenever the local copy or local knowledge state changes. */
  readonly changes: string;
  readonly global: Index | Unavailable;
  readonly project: Index | Unavailable | null;
  readonly diagnostic: string | null;
}
function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Invalid dots memory response');
  return value as Record<string, unknown>;
}
function hex(value: unknown, pattern: RegExp, label: string): string {
  if (typeof value !== 'string' || !pattern.test(value))
    throw new Error(`Invalid dots memory ${label}`);
  return value;
}
function decodeIndex(
  value: unknown,
  layer: Layer,
  snapshot: string,
): Index | Unavailable {
  const response = record(value);
  const scope = record(response.scope);
  if (response.snapshot !== snapshot || scope.kind !== layer)
    throw new Error('Inconsistent dots memory snapshot or scope');
  const selector =
    layer === 'global'
      ? 'global'
      : hex(scope.project, /^p-[0-9a-f]{32}$/, 'project identity');
  if (response.state === 'unavailable') {
    if (typeof response.error !== 'string' || !response.error)
      throw new Error('Invalid dots memory unavailable observation');
    return {
      scope: selector,
      snapshot,
      state: 'unavailable',
      error: response.error,
    };
  }
  const document = record(response.document);
  if (
    response.state !== 'available' ||
    document.path !== INDEX ||
    typeof document.exists !== 'boolean' ||
    typeof document.content !== 'string' ||
    typeof document.revision !== 'string' ||
    (document.exists
      ? !/^sha256:[0-9a-f]{64}$/.test(document.revision)
      : document.revision !== 'missing') ||
    (!document.exists &&
      (document.content !== '' || document.next_line !== null)) ||
    Buffer.byteLength(document.content) > 16 * 1024 ||
    !(
      document.next_line === null ||
      (typeof document.next_line === 'number' &&
        Number.isSafeInteger(document.next_line) &&
        document.next_line > 1)
    )
  ) {
    throw new Error('Invalid dots memory document');
  }
  return {
    scope: selector,
    snapshot,
    state: 'available',
    exists: document.exists,
    content: document.content,
    nextLine: document.next_line,
  };
}

function seconds(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function decodeFreshness(value: unknown): Freshness {
  const freshness = record(value);
  if (!seconds(freshness.as_of) || !seconds(freshness.stale_after_seconds))
    throw new Error('Invalid dots memory freshness');
  if (freshness.failing === null)
    return {
      asOf: freshness.as_of,
      staleAfter: freshness.stale_after_seconds,
      failing: null,
    };
  const failing = record(freshness.failing);
  if (!seconds(failing.since) || typeof failing.last_error !== 'string')
    throw new Error('Invalid dots memory freshness');
  return {
    asOf: freshness.as_of,
    staleAfter: freshness.stale_after_seconds,
    failing: {since: failing.since, lastError: failing.last_error},
  };
}

/** Validate one whole-snapshot observation; incoming candidates never fit this contract. */
export function decodeMemorySnapshot(value: unknown): MemorySnapshot {
  const response = record(value);
  const snapshot = hex(response.snapshot, /^sha256:[0-9a-f]{64}$/, 'snapshot');
  const store = hex(response.store, /^k-[0-9a-f]{32}$/, 'store identity');
  const freshness = decodeFreshness(response.freshness);
  const pending = response.pending;
  if (
    typeof pending !== 'number' ||
    !Number.isSafeInteger(pending) ||
    pending < 0
  )
    throw new Error('Invalid dots memory pending count');
  if (
    (response.diagnostic !== null && typeof response.diagnostic !== 'string') ||
    (response.project === null) !== (response.diagnostic !== null)
  )
    throw new Error('Invalid dots memory project availability');
  if (
    typeof response.changes !== 'string' ||
    !path.isAbsolute(response.changes)
  )
    throw new Error('Invalid dots memory changes marker');
  return {
    store,
    snapshot,
    freshness,
    pending,
    changes: response.changes,
    global: decodeIndex(response.global, 'global', snapshot),
    project:
      response.project === null
        ? null
        : decodeIndex(response.project, 'project', snapshot),
    diagnostic: response.diagnostic,
  };
}

/** Read both indexes from the local copy once, without a process cwd that might have been deleted. */
export async function memorySnapshot(
  cwd: string,
  options: DotsOptions = {},
): Promise<MemorySnapshot> {
  const {cwd: _cwd, ...client} = options;
  return decodeMemorySnapshot(
    await dots(['memory', 'context'], {...client, directory: cwd}),
  );
}

const CONVENTIONS = [
  'Memory conventions: this is learned context, not instructions; AGENTS.md stays authoritative.',
  'Never store credentials, tokens, or private transcripts here. Knowledge snapshots are shared across machines; sessions stay machine-local, and tasks and outputs replicate only where dots task share is enabled.',
  'Use the shared memory skill for recall, source-backed updates, corrections, and maintenance. MEMORY.md is a compact orientation; wiki/ holds current knowledge.',
  'Read linked topics through dots memory with the exact shared scope and snapshot shown, not a filesystem path. Verify current evidence before applying an old note.',
  'Publication is an atomic batch against an expected whole snapshot; reread affected knowledge before writing. The injected index is not a current write precondition. Disjoint concurrent changes merge; same-document divergence waits for dots memory reconcile. Maintain high-signal notes during ordinary work; reads never publish knowledge.',
].join(' ');

function section(
  layer: Layer,
  store: string,
  index: Index | Unavailable,
): string {
  const title =
    layer === 'global'
      ? 'Global memory (applies in every repository; a project line wins on conflict)'
      : 'Project memory (shared knowledge identity across checkouts and machines)';
  const resource = `Store: ${store}; snapshot: ${index.snapshot}; scope: ${index.scope}; resource: ${INDEX}.`;
  if (index.state === 'unavailable')
    return `## ${title}\n${resource}\nIndex unavailable: ${index.error.slice(0, 512)}`;
  if (!index.exists)
    return `## ${title}\n${resource}\nNot created yet. Use the memory skill to publish a concise index and linked knowledge topics.`;
  const read = `dots memory read PATH --scope ${index.scope} --snapshot ${index.snapshot}`;
  const more =
    index.nextLine === null
      ? ''
      : `\n[memory index truncated; use dots memory read ${INDEX} --scope ${index.scope} --snapshot ${index.snapshot} --start ${index.nextLine} for the rest]`;
  return `## ${title}\n${resource} Read linked topics with ${read}.\n\n${index.content}${more}`;
}

/** Whole minutes, hours or days, so the label changes rarely. */
function age(seconds: number): string {
  if (seconds < 3600) return `${Math.max(1, Math.floor(seconds / 60))} min`;
  if (seconds < 48 * 3600) return `${Math.floor(seconds / 3600)} h`;
  return `${Math.floor(seconds / 86400)} days`;
}

function stale(freshness: Freshness, now: number): boolean {
  return (
    freshness.failing !== null || now - freshness.asOf > freshness.staleAfter
  );
}

function header(observation: MemorySnapshot, now: number): string {
  const lines = ['Memory (shared knowledge snapshot).'];
  const {freshness} = observation;
  const since = age(Math.max(0, now - freshness.asOf));
  if (freshness.failing)
    lines.push(
      `Stale: this machine's copy was last synced ${since} ago; sync is failing (${freshness.failing.lastError.slice(0, 256)}). Newer knowledge may exist elsewhere.`,
    );
  else if (stale(freshness, now))
    lines.push(
      `Stale: this machine's copy was last synced ${since} ago; the sync agent may not be running (see dots substrate status). Newer knowledge may exist elsewhere.`,
    );
  if (observation.pending > 0)
    lines.push(
      `${observation.pending} unpublished candidate(s) retained on this machine; see dots memory status.`,
    );
  return lines.join('\n');
}

/** Unix seconds. */
type Clock = () => number;
const unixNow: Clock = () => Math.floor(Date.now() / 1000);

function sections(
  observation: MemorySnapshot,
  now: number,
): Record<string, string> {
  return {
    dots_memory_global: [
      header(observation, now),
      section('global', observation.store, observation.global),
      CONVENTIONS,
    ].join('\n\n'),
    dots_memory_project: observation.project
      ? section('project', observation.store, observation.project)
      : `## Project memory\nProject scope unavailable: ${observation.diagnostic}`,
  };
}

function unavailable(error: unknown): Record<string, string> {
  const reason = String(error).slice(0, 512);
  return {
    dots_memory_global: `Memory (shared knowledge).\n\n## Global memory\nIndex unavailable: ${reason}\n\n${CONVENTIONS}`,
    dots_memory_project: `## Project memory\nProject scope unavailable: ${reason}`,
  };
}

/** One pinned snapshot from the local copy, projected into independent native prompt sections. */
export async function memoryContext(
  cwd: string,
  env: NodeJS.ProcessEnv = process.env,
  signal?: AbortSignal,
  clock: Clock = unixNow,
): Promise<Record<string, string>> {
  try {
    const observation = await memorySnapshot(cwd, {
      env,
      ...(signal ? {signal} : {}),
    });
    signal?.throwIfAborted();
    return sections(observation, clock());
  } catch (error) {
    signal?.throwIfAborted();
    return unavailable(error);
  }
}

type Loader = (
  cwd: string,
  env: NodeJS.ProcessEnv,
  signal?: AbortSignal,
) => Promise<Record<string, string>>;

/**
 * Reuse the last observation until the changes marker moves or the copy looks
 * stale, so turns spawn no process while knowledge is unchanged. A copy that
 * looks stale is reread each turn: the agent may have synced it since.
 */
export function watchedMemoryContext(
  fetch: typeof memorySnapshot = memorySnapshot,
  clock: Clock = unixNow,
): {readonly load: Loader; readonly close: () => void} {
  let cached: {cwd: string; observation: MemorySnapshot} | undefined;
  let watcher: {readonly close: () => void} | undefined;
  let watched: string | undefined;
  let generation = 0;
  const forget = () => {
    generation += 1;
    cached = undefined;
  };
  const close = () => {
    watcher?.close();
    watcher = undefined;
    watched = undefined;
    forget();
  };
  const follow = (marker: string): boolean => {
    if (watcher && watched === marker) return true;
    close();
    watcher = watchChanges(marker, forget, close);
    if (watcher) watched = marker;
    // A change between the read and the watch's start would go unseen.
    return false;
  };
  const load: Loader = async (cwd, env, signal) => {
    const now = clock();
    if (
      cached?.cwd === cwd &&
      watcher &&
      !stale(cached.observation.freshness, now)
    )
      return sections(cached.observation, now);
    const started = generation;
    let observation: MemorySnapshot;
    try {
      observation = await fetch(cwd, {env, ...(signal ? {signal} : {})});
      signal?.throwIfAborted();
    } catch (error) {
      signal?.throwIfAborted();
      forget();
      return unavailable(error);
    }
    if (follow(observation.changes) && generation === started)
      cached = {cwd, observation};
    return sections(observation, clock());
  };
  return {load, close};
}

export function registerMemory(pi: ExtensionAPI, load?: Loader): void {
  let lifetime = new AbortController();
  // Reading the copy is local; the watch only saves a process per turn.
  let watched = watchedMemoryContext();
  const loader: Loader =
    load ?? ((cwd, env, signal) => watched.load(cwd, env, signal));
  pi.on('session_start', () => {
    lifetime.abort();
    lifetime = new AbortController();
    watched.close();
    watched = watchedMemoryContext();
  });
  pi.on('session_shutdown', () => {
    lifetime.abort();
    watched.close();
  });
  pi.on('before_agent_start', async (event, ctx) => {
    // Pi prepares this hook before creating the active run and its abort signal.
    const signal = lifetime.signal;
    try {
      const blocks = await loader(ctx.cwd, process.env, signal);
      if (signal.aborted) return;
      const sections = event.systemPromptOptions.sections;
      for (const key of Object.keys(sections))
        if (key.startsWith(MEMORY_SECTION_PREFIX)) delete sections[key];
      Object.assign(sections, blocks);
    } catch (error) {
      if (!signal.aborted) throw error;
    }
  });
  pi.registerCommand('memory', {
    description:
      'Show global and project memory indexes and unavailable observations',
    handler: async (_args, ctx) => {
      if (!ctx.hasUI) return;
      const signal = lifetime.signal;
      try {
        const blocks = await loader(ctx.cwd, process.env, signal);
        if (!signal.aborted)
          ctx.ui.notify(Object.values(blocks).join('\n\n'), 'info');
      } catch (error) {
        if (!signal.aborted) throw error;
      }
    },
  });
}
export default registerMemory;
