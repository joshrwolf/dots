import path from 'node:path';
import type {ExtensionAPI} from '@earendil-works/pi-coding-agent';
import {Type, type Static, type TSchema} from 'typebox';
import {Check} from 'typebox/value';
import {systemClock, type Clock} from './clock.ts';
import {NdjsonStream, type Launch, type StreamState} from './stream.ts';

/**
 * The companion's one owner of Rex observation: CLI resolution, snapshots
 * through the control API, and one `rex events --all --json` stream per host,
 * filtered in process. Observers never autostart a server and never attach a
 * streaming client to a user's session.
 */

export const REX_TERMINAL = 'com.superlogical.terminal';

/** The Rex CLI: Rex puts its own bin directory in each terminal's environment. */
export function rexBinary(env: NodeJS.ProcessEnv = process.env): string {
  return env.REX_BIN_DIR ? path.join(env.REX_BIN_DIR, 'rex') : 'rex';
}

export interface ExecResult {
  readonly stdout: string;
  readonly stderr: string;
  readonly code: number;
  readonly killed?: boolean;
}
/** Runs a program to completion; the test seam for snapshots. */
export type Exec = (
  file: string,
  args: readonly string[],
  signal: AbortSignal,
) => Promise<ExecResult>;

/** The host's process runner owns the deadline, output cap and kill escalation. */
export function hostExec(pi: Pick<ExtensionAPI, 'exec'>): Exec {
  return (file, args, signal) =>
    pi.exec(file, [...args], {
      signal,
      timeout: 15_000,
      maxBuffer: 4 * 1024 * 1024,
    });
}

/**
 * A failed Rex call. `code` is the API's error code, such as `not_found`, when
 * the server answered; it is absent when the server could not be reached.
 */
export class RexError extends Error {
  readonly code: string | undefined;
  constructor(message: string, code?: string) {
    super(message);
    this.name = 'RexError';
    this.code = code;
  }
}

const Owner = Type.Object({
  pid: Type.Integer(),
  name: Type.Optional(Type.String()),
});
const StatusRecordSchema = Type.Object({
  id: Type.Optional(Type.String()),
  state: Type.String(),
  kind: Type.Optional(Type.String()),
  app: Type.Optional(Type.String()),
  effective_app: Type.Optional(Type.String()),
  title: Type.Optional(Type.String()),
  msg: Type.Optional(Type.String()),
  progress: Type.Optional(Type.Number()),
  updated_at: Type.String(),
  owner: Type.Optional(Owner),
});
type WireRecord = Static<typeof StatusRecordSchema>;

/** A program status record (OSC 7501). Text fields are program-controlled, untrusted data. */
export interface StatusRecord {
  readonly state: string;
  readonly kind?: string;
  readonly app?: string;
  readonly title?: string;
  readonly msg?: string;
  readonly progress?: number;
  /** Opaque: compared for identity, never ordered. */
  readonly updatedAt: string;
  readonly owner?: {readonly pid: number; readonly name?: string};
}

function statusRecord(wire: WireRecord): StatusRecord {
  const app = wire.app ?? wire.effective_app;
  return {
    state: wire.state,
    updatedAt: wire.updated_at,
    ...(wire.kind === undefined ? {} : {kind: wire.kind}),
    ...(app === undefined ? {} : {app}),
    ...(wire.title === undefined ? {} : {title: wire.title}),
    ...(wire.msg === undefined ? {} : {msg: wire.msg}),
    ...(wire.progress === undefined ? {} : {progress: wire.progress}),
    ...(wire.owner
      ? {
          owner: {
            pid: wire.owner.pid,
            ...(wire.owner.name === undefined ? {} : {name: wire.owner.name}),
          },
        }
      : {}),
  };
}

export interface ExitStatus {
  readonly code: number;
  readonly error?: string;
}
export interface ProcessInfo {
  readonly pid: number;
  readonly name: string;
}

/** A terminal event or lifecycle change, already filtered to what observers use. */
export type RexEvent =
  | {
      readonly type: 'program_status_changed';
      readonly session: string;
      readonly block: string;
      /** Empty for the root record. */
      readonly id: string;
      readonly record: StatusRecord;
    }
  | {
      readonly type: 'program_status_removed';
      readonly session: string;
      readonly block: string;
      readonly ids: readonly string[];
      readonly reason?: string;
    }
  | {
      readonly type: 'child_exited';
      readonly session: string;
      readonly block: string;
      readonly exit: ExitStatus;
    }
  | {
      readonly type: 'block_closed';
      readonly session: string;
      readonly block: string;
    }
  | {readonly type: 'session_destroyed'; readonly session: string};

const Envelope = Type.Object({type: Type.String(), payload: Type.Unknown()});
const SessionEvent = Type.Object({
  session_id: Type.String(),
  event: Envelope,
});
const BlockEvent = Type.Object({
  block_id: Type.String(),
  name: Type.String(),
  payload: Type.Unknown(),
});
const StatusChanged = Type.Object({
  id: Type.String(),
  record: StatusRecordSchema,
});
const StatusRemoved = Type.Object({
  ids: Type.Array(Type.String()),
  reason: Type.Optional(Type.String()),
});
const ChildExited = Type.Object({
  exit_code: Type.Integer(),
  error: Type.Optional(Type.String()),
});
const BlockId = Type.Object({block_id: Type.String()});
const SessionId = Type.Object({session_id: Type.String()});

const is = <T extends TSchema>(schema: T, value: unknown): value is Static<T> =>
  Check(schema, value);

/** One line of `rex events --all --json`, or undefined for events observers ignore. */
export function parseRexEvent(value: unknown): RexEvent | undefined {
  if (!is(Envelope, value)) return undefined;
  if (value.type === 'session_destroyed')
    return is(SessionId, value.payload)
      ? {type: 'session_destroyed', session: value.payload.session_id}
      : undefined;
  if (value.type !== 'session_event' || !is(SessionEvent, value.payload))
    return undefined;
  const session = value.payload.session_id;
  const inner = value.payload.event;
  if (inner.type === 'block_closed')
    return is(BlockId, inner.payload)
      ? {type: 'block_closed', session, block: inner.payload.block_id}
      : undefined;
  if (inner.type !== 'block_event' || !is(BlockEvent, inner.payload))
    return undefined;
  const block = inner.payload.block_id;
  const payload = inner.payload.payload;
  switch (inner.payload.name) {
    case 'program_status_changed':
      return is(StatusChanged, payload)
        ? {
            type: 'program_status_changed',
            session,
            block,
            id: payload.id,
            record: statusRecord(payload.record),
          }
        : undefined;
    case 'program_status_removed':
      return is(StatusRemoved, payload)
        ? {
            type: 'program_status_removed',
            session,
            block,
            ids: payload.ids,
            ...(payload.reason ? {reason: payload.reason} : {}),
          }
        : undefined;
    case 'child_exited':
      return is(ChildExited, payload)
        ? {
            type: 'child_exited',
            session,
            block,
            exit: {
              code: payload.exit_code,
              ...(payload.error ? {error: payload.error} : {}),
            },
          }
        : undefined;
    default:
      return undefined;
  }
}

/** Where a pane lives. `host` is a Rex host label or address; absent means the server the CLI reaches. */
export interface PaneRef {
  readonly host?: string;
  readonly session: string;
  readonly block: string;
}

export interface PaneBinding extends PaneRef {
  readonly sessionLabel: string;
  readonly label: string;
  readonly where: string;
}

export type PaneSnapshot =
  | {
      readonly present: true;
      /** The server run that answered, from `server.status`. */
      readonly instance: string;
      /** The root status record, if any. */
      readonly record?: StatusRecord;
      /** How the block's own program exited; null while it runs. */
      readonly exit: ExitStatus | null;
      readonly child?: ProcessInfo;
      readonly foreground?: ProcessInfo;
    }
  | {
      readonly present: false;
      readonly instance: string;
      readonly reason: 'block closed' | 'session destroyed';
    };

export interface RexListener {
  /** A new connection: anything since the previous one was not observed. */
  open(): void;
  /** Reconcile now: after the first line on a connection, or a short settle without one. */
  sync(): void;
  event(event: RexEvent): void;
  state(state: StreamState): void;
}

export interface RexOptions {
  readonly env?: NodeJS.ProcessEnv;
  readonly exec: Exec;
  readonly launch?: Launch;
  readonly clock?: Clock;
  /** `--all` has no ready marker: snapshot again after this long without a line. */
  readonly settleMs?: number;
}

const Instance = Type.Object({instance_id: Type.String({minLength: 1})});
const Candidate = Type.Object({
  kind: Type.String(),
  label: Type.String(),
  session_id: Type.Optional(Type.String()),
  block_id: Type.Optional(Type.String()),
  where: Type.Optional(Type.String()),
});
const Resolved = Type.Object({
  results: Type.Array(
    Type.Object({
      query: Type.String(),
      status: Type.String(),
      reason: Type.Optional(Type.String()),
      target: Type.Optional(Type.Union([Type.Null(), Candidate])),
      candidates: Type.Optional(
        Type.Union([Type.Null(), Type.Array(Candidate)]),
      ),
    }),
    {minItems: 1},
  ),
});
const Blocks = Type.Object({
  blocks: Type.Array(
    Type.Object({
      block_id: Type.String(),
      label: Type.String(),
      creator_name: Type.String(),
    }),
  ),
});
const Sessions = Type.Object({
  sessions: Type.Array(
    Type.Object({session_id: Type.String(), label: Type.String()}),
  ),
});
const Records = Type.Object({records: Type.Array(StatusRecordSchema)});
const Proc = Type.Object({pid: Type.Integer(), name: Type.String()});
const ProcessSchema = Type.Object({
  last_exit: Type.Union([Type.Null(), ChildExited]),
  child: Type.Optional(Type.Union([Type.Null(), Proc])),
  foreground: Type.Optional(Type.Union([Type.Null(), Proc])),
});

const shortId = (id: string) => id.slice(-8);
const LIST = 20;
function listed(items: readonly string[]): string {
  return (
    items.slice(0, LIST).join(', ') +
    (items.length > LIST ? `, and ${items.length - LIST} more` : '')
  );
}

interface Watch {
  readonly stream: NdjsonStream;
  readonly listeners: Set<RexListener>;
  unsubscribe: (() => void) | undefined;
  settle: unknown;
  lined: boolean;
}

export class Rex {
  private readonly binary: string;
  private readonly exec: Exec;
  private readonly launch: Launch | undefined;
  private readonly clock: Clock;
  private readonly settleMs: number;
  private readonly watches = new Map<string, Watch>();
  private readonly calls = new Set<{
    controller: AbortController;
    done: Promise<unknown>;
  }>();

  constructor(options: RexOptions) {
    this.binary = rexBinary(options.env);
    this.exec = options.exec;
    this.launch = options.launch;
    this.clock = options.clock ?? systemClock;
    this.settleMs = options.settleMs ?? 2_000;
  }

  private args(host: string | undefined, rest: readonly string[]): string[] {
    return ['--autostart=false', ...(host ? ['--server', host] : []), ...rest];
  }

  /** One control API call; its result is untrusted JSON for the caller to validate. */
  async call(
    host: string | undefined,
    method: string,
    params: object = {},
  ): Promise<unknown> {
    const controller = new AbortController();
    const done = this.exec(
      this.binary,
      this.args(host, ['api', 'call', method, JSON.stringify(params)]),
      controller.signal,
    );
    const call = {controller, done};
    this.calls.add(call);
    let result: ExecResult;
    try {
      result = await done;
    } catch (error) {
      throw new RexError(
        `rex ${method}: ${error instanceof Error ? error.message : String(error)}`,
      );
    } finally {
      this.calls.delete(call);
    }
    if (result.killed || result.code !== 0) {
      const stderr = result.stderr.trim();
      const code = /\bcode=([a-z_]+)\)/.exec(stderr)?.[1];
      throw new RexError(
        stderr || `rex ${method} failed${result.killed ? ' (timed out)' : ''}`,
        result.killed ? undefined : code,
      );
    }
    try {
      return JSON.parse(result.stdout);
    } catch {
      throw new RexError(`rex ${method} returned invalid JSON`);
    }
  }

  private async typed<T extends TSchema>(
    host: string | undefined,
    method: string,
    params: object,
    schema: T,
  ): Promise<Static<T>> {
    const value = await this.call(host, method, params);
    if (!is(schema, value))
      throw new RexError(`rex ${method} returned an unexpected shape`);
    return value;
  }

  /** The server run's identity; a new value means its programs are gone. */
  async instance(host?: string): Promise<string> {
    return (await this.typed(host, 'server.status', {}, Instance)).instance_id;
  }

  /** Resolve a target in a session to one terminal block, or explain why not. */
  async resolvePane(spec: {
    target: string;
    session: string;
    host?: string;
  }): Promise<PaneBinding> {
    const {host} = spec;
    const [session] = (
      await this.typed(
        host,
        'target.resolve',
        {kinds: ['session'], queries: [spec.session]},
        Resolved,
      )
    ).results;
    if (session?.status !== 'resolved' || !session.target?.session_id) {
      if (session?.status === 'ambiguous')
        throw new RexError(
          `Rex session ${JSON.stringify(spec.session)} is ambiguous: ${listed(
            (session.candidates ?? []).map(
              c => `${c.label || '(no label)'} (${c.session_id ?? '?'})`,
            ),
          )}`,
        );
      const {sessions} = await this.typed(host, 'session.list', {}, Sessions);
      throw new RexError(
        `no Rex session ${JSON.stringify(spec.session)}; sessions: ${
          listed(sessions.map(s => `${s.label} (${shortId(s.session_id)})`)) ||
          'none'
        }`,
      );
    }
    const sessionId = session.target.session_id;
    const sessionLabel = session.target.label;
    const [resolved, {blocks}] = await Promise.all([
      this.typed(
        host,
        'target.resolve',
        {
          kinds: ['block'],
          queries: [spec.target],
          scope: {session: sessionId},
        },
        Resolved,
      ),
      this.typed(host, 'session.list_blocks', {session_id: sessionId}, Blocks),
    ]);
    const [block] = resolved.results;
    const describe = (id: string, label: string) =>
      `${label || '(no label)'} (${shortId(id)})`;
    if (block?.status !== 'resolved' || !block.target?.block_id) {
      if (block?.status === 'ambiguous')
        throw new RexError(
          `target ${JSON.stringify(spec.target)} matches several blocks in ${sessionLabel}: ${listed(
            (block.candidates ?? []).map(
              c =>
                `${describe(c.block_id ?? '', c.label)}${c.where ? ` in ${c.where}` : ''}`,
            ),
          )}`,
        );
      throw new RexError(
        `no block ${JSON.stringify(spec.target)} in session ${sessionLabel}${
          block?.reason ? ` (${block.reason})` : ''
        }; blocks: ${listed(blocks.map(b => describe(b.block_id, b.label))) || 'none'}`,
      );
    }
    const blockId = block.target.block_id;
    const listing = blocks.find(b => b.block_id === blockId);
    if (listing && listing.creator_name !== REX_TERMINAL)
      throw new RexError(
        `block ${describe(blockId, listing.label)} is a ${listing.creator_name} block, not a terminal`,
      );
    return {
      ...(host ? {host} : {}),
      session: sessionId,
      sessionLabel,
      block: blockId,
      label: block.target.label,
      where: block.target.where ?? '',
    };
  }

  /** The pane's current facts. Throws RexError when Rex cannot be reached. */
  async snapshotPane(ref: PaneRef): Promise<PaneSnapshot> {
    const target = {session_id: ref.session, block_id: ref.block};
    const [instance, status, proc] = await Promise.allSettled([
      this.instance(ref.host),
      this.typed(
        ref.host,
        'block.method',
        {...target, method: 'program_status'},
        Records,
      ),
      this.typed(
        ref.host,
        'block.method',
        {...target, method: 'process'},
        ProcessSchema,
      ),
    ]);
    if (instance.status === 'rejected') throw instance.reason;
    for (const result of [status, proc]) {
      if (result.status === 'fulfilled') continue;
      const error: unknown = result.reason;
      if (error instanceof RexError && error.code === 'session_not_found')
        return {
          present: false,
          instance: instance.value,
          reason: 'session destroyed',
        };
      if (error instanceof RexError && error.code === 'not_found')
        return {
          present: false,
          instance: instance.value,
          reason: 'block closed',
        };
    }
    if (status.status === 'rejected') throw status.reason;
    if (proc.status === 'rejected') throw proc.reason;
    const root = status.value.records.find(record => (record.id ?? '') === '');
    const {last_exit, child, foreground} = proc.value;
    return {
      present: true,
      instance: instance.value,
      ...(root ? {record: statusRecord(root)} : {}),
      exit: last_exit
        ? {
            code: last_exit.exit_code,
            ...(last_exit.error ? {error: last_exit.error} : {}),
          }
        : null,
      ...(child ? {child: {pid: child.pid, name: child.name}} : {}),
      ...(foreground
        ? {foreground: {pid: foreground.pid, name: foreground.name}}
        : {}),
    };
  }

  /** Follow every event on a host. The host's one stream runs while any listener remains. */
  watch(host: string | undefined, listener: RexListener): () => void {
    const key = host ?? '';
    let watch = this.watches.get(key);
    if (!watch) {
      const created: Watch = {
        stream: new NdjsonStream({
          file: this.binary,
          args: this.args(host, ['events', '--all', '--json']),
          clock: this.clock,
          ...(this.launch ? {launch: this.launch} : {}),
        }),
        listeners: new Set(),
        unsubscribe: undefined,
        settle: undefined,
        lined: false,
      };
      watch = created;
      this.watches.set(key, watch);
    }
    const current = watch;
    current.listeners.add(listener);
    if (!current.unsubscribe) {
      current.unsubscribe = current.stream.subscribe({
        open: () => {
          current.lined = false;
          this.clearSettle(current);
          current.settle = this.clock.setTimeout(() => {
            current.settle = undefined;
            if (current.lined) return;
            current.lined = true;
            for (const each of [...current.listeners]) each.sync();
          }, this.settleMs);
          for (const each of [...current.listeners]) each.open();
        },
        line: value => {
          const event = parseRexEvent(value);
          if (event)
            for (const each of [...current.listeners]) each.event(event);
          if (current.lined) return;
          current.lined = true;
          this.clearSettle(current);
          for (const each of [...current.listeners]) each.sync();
        },
        state: state => {
          if (state.status !== 'connected') this.clearSettle(current);
          for (const each of [...current.listeners]) each.state(state);
        },
      });
    } else if (current.stream.state.status !== 'idle')
      listener.state(current.stream.state);
    return () => {
      if (!current.listeners.delete(listener) || current.listeners.size) return;
      this.clearSettle(current);
      current.unsubscribe?.();
      current.unsubscribe = undefined;
    };
  }

  private clearSettle(watch: Watch): void {
    if (watch.settle !== undefined) this.clock.clearTimeout(watch.settle);
    watch.settle = undefined;
  }

  /** Stop every stream and call; resolves once their processes have exited. */
  async close(): Promise<void> {
    const streams = [...this.watches.values()];
    this.watches.clear();
    for (const watch of streams) {
      this.clearSettle(watch);
      watch.listeners.clear();
      watch.unsubscribe = undefined;
    }
    const calls = [...this.calls];
    for (const call of calls) call.controller.abort();
    await Promise.allSettled([
      ...streams.map(watch => watch.stream.close()),
      ...calls.map(call => call.done),
    ]);
  }
}
